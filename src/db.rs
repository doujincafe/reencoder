use anyhow::{Result, anyhow};
use directories::BaseDirs;
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::flac::{CURRENT_VENDOR, get_vendor};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY UNIQUE, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_NEW_ITEM: &str = "INSERT INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const REPLACE_ITEM: &str = "REPLACE INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const TOENCODE_QUERY: &str = "SELECT path FROM flacs WHERE toencode";
const TOENCODE_NUMBER: &str = "SELECT COUNT(*) from flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = ?1)";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flacs WHERE path = ?1";
const DEDUPE_DB: &str =
    "DELETE FROM flacs WHERE rowid NOT IN (SELECT MAX(rowid) FROM flacs GROUP BY path)";
const GET_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = ?1";

pub fn open_db(path: Option<impl AsRef<Path>>) -> Result<Pool<SqliteConnectionManager>> {
    if let Some(file) = path {
        let manager = SqliteConnectionManager::file(file);
        let pool = Pool::builder().build(manager)?;
        let conn = pool.get()?;
        conn.execute(TABLE_CREATE, ())?;
        Ok(pool)
    } else if let Some(base_dir) = BaseDirs::new() {
        let file = Path::new(base_dir.data_dir()).join("reencoder.db");
        let manager = SqliteConnectionManager::file(file);
        let pool = Pool::builder().build(manager)?;
        let conn = pool.get()?;
        conn.execute(TABLE_CREATE, ())?;
        Ok(pool)
    } else {
        Err(anyhow!("Failed to locate data directory"))
    }
}

pub struct Database(pub PooledConnection<SqliteConnectionManager>);

impl Database {
    pub fn new(conn: PooledConnection<SqliteConnectionManager>) -> Self {
        Database(conn)
    }

    pub fn insert_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        let toencode = !matches!(get_vendor(&filename)?.as_str(), CURRENT_VENDOR);

        let modtime = filename
            .as_ref()
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.0.execute(
            ADD_NEW_ITEM,
            params![filename.as_ref().to_str().unwrap(), toencode, modtime],
        )?;

        Ok(())
    }

    pub fn update_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        let modtime = filename
            .as_ref()
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.0.execute(
            REPLACE_ITEM,
            params![filename.as_ref().to_str().unwrap(), false, modtime],
        )?;

        Ok(())
    }

    pub fn check_file(&self, filename: impl AsRef<Path>) -> Result<bool> {
        if self.0.query_one(
            CHECK_FILE,
            params!(filename.as_ref().to_str().unwrap()),
            |row| {
                let num: bool = row.get(0)?;
                Ok(num)
            },
        )? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn init_clean_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        self.0.execute(DEDUPE_DB, ())?;
        let mut stmt = self.0.prepare(FETCH_FILES)?;
        let mut rows = stmt.query(())?;
        let mut files = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let path: String = row.get(0)?;
            files.push(PathBuf::from(path));
        }
        Ok(files)
    }

    pub fn remove_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        self.0
            .execute(REMOVE_FILE, params!(filename.as_ref().to_str().unwrap()))?;
        Ok(())
    }

    pub fn get_toencode_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self.0.prepare(TOENCODE_QUERY)?;
        let mut rows = stmt.query(())?;
        let mut files: Vec<PathBuf> = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let path: String = row.get(0)?;
            files.push(PathBuf::from(path));
        }
        Ok(files)
    }

    pub fn get_toencode_number(&self) -> Result<u64, rusqlite::Error> {
        self.0.query_one(TOENCODE_NUMBER, (), |row| {
            let num: u64 = row.get(0)?;
            Ok(num)
        })
    }

    pub fn get_modtime(&self, file: impl AsRef<Path>) -> Result<u64> {
        Ok(self.0.query_one(
            GET_MODTIME,
            params![file.as_ref().to_str().unwrap()],
            |row| {
                let modtime: u64 = row.get(0)?;
                Ok(modtime)
            },
        )?)
    }

    pub fn vaccum(&self) -> Result<()> {
        self.0.execute("VACUUM", ())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn check_localfiles() {
        let dbname = String::from("temp1.db");
        let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
        let mut counter = 0;
        let pool = open_db(Some(&dbname)).unwrap();
        let conn = Database::new(pool.get().unwrap());
        for file in filenames {
            let _ = conn.insert_file(&file.to_string());
        }
        let mut stmt = conn.0.prepare(TOENCODE_QUERY).unwrap();
        let mut returned = stmt.query(()).unwrap();

        while let Ok(Some(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }

    #[test]
    fn check_update() {
        let dbname = String::from("temp2.db");
        let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
        let pool = open_db(Some(&dbname)).unwrap();
        let conn = Database::new(pool.get().unwrap());
        for file in filenames {
            let _ = conn.insert_file(Path::new(file).canonicalize().unwrap());
        }

        let _ = conn.0.execute(
            REPLACE_ITEM,
            params![
                Path::new("16bit.flac")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                true,
                ""
            ],
        );

        conn.update_file(
            Path::new("16bit.flac")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();

        let mut stmt = conn.0.prepare(TOENCODE_QUERY).unwrap();
        let mut returned = stmt.query(()).unwrap();
        let mut counter = 0;
        while let Ok(Some(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }
}
