use anyhow::{Result, anyhow};
use directories::BaseDirs;
use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::flac::{CURRENT_VENDOR, get_vendor};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY UNIQUE, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_ITEM: &str = "INSERT INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const UPDATE_ITEM: &str = "UPDATE flacs SET toencode = ?2, modtime = ?3 WHERE path = ?1";
const TOENCODE_PATHS: &str = "SELECT path FROM flacs WHERE toencode";
const TOENCODE_NUMBER: &str = "SELECT COUNT(*) from flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = ?1)";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flacs WHERE path = ?1";
const DEDUPE_DB: &str =
    "DELETE FROM flacs WHERE rowid NOT IN (SELECT MAX(rowid) FROM flacs GROUP BY path)";
const GET_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = ?1";

pub trait Database {
    type Conn;
    fn new(path: Option<&PathBuf>) -> Result<Self::Conn>;
    fn insert_file(&self, filename: &Path) -> Result<()>;
    fn update_file(&self, filename: &Path) -> Result<()>;
    fn check_file(&self, filename: &Path) -> Result<bool>;
    fn init_clean_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error>;
    fn remove_file(&self, filename: &Path) -> Result<()>;
    fn get_toencode_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error>;
    fn get_toencode_number(&self) -> Result<u64, rusqlite::Error>;
    fn get_modtime(&self, file: &Path) -> Result<u64>;
    fn vacuum(&self) -> Result<()>;
}

impl Database for Connection {
    type Conn = Connection;
    fn new(path: Option<&PathBuf>) -> Result<Self> {
        let conn = if let Some(file) = path {
            Connection::open(file)?
        } else if let Some(base_dir) = BaseDirs::new() {
            let file = Path::new(base_dir.data_dir()).join("reencoder.db");
            Connection::open(file)?
        } else {
            return Err(anyhow!("Failed to locate data directory"));
        };
        conn.execute(TABLE_CREATE, ())?;
        Ok(conn)
    }

    fn insert_file(&self, filename: &Path) -> Result<()> {
        let toencode = !matches!(get_vendor(&filename)?.as_str(), CURRENT_VENDOR);

        let modtime = filename
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.execute(
            ADD_ITEM,
            params![filename.to_str().unwrap(), toencode, modtime],
        )?;

        Ok(())
    }

    fn update_file(&self, filename: &Path) -> Result<()> {
        let modtime = filename
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.execute(
            UPDATE_ITEM,
            params![filename.to_str().unwrap(), false, modtime],
        )?;

        Ok(())
    }

    fn check_file(&self, filename: &Path) -> Result<bool> {
        if self.query_one(CHECK_FILE, params!(filename.to_str().unwrap()), |row| {
            let num: bool = row.get(0)?;
            Ok(num)
        })? {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn init_clean_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        self.execute(DEDUPE_DB, ())?;
        let mut stmt = self.prepare(FETCH_FILES)?;
        let mut rows = stmt.query(())?;
        let mut files = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let path: String = row.get(0)?;
            files.push(PathBuf::from(path));
        }
        Ok(files)
    }

    fn remove_file(&self, filename: &Path) -> Result<()> {
        self.execute(REMOVE_FILE, params!(filename.to_str().unwrap()))?;
        Ok(())
    }

    fn get_toencode_files(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self.prepare(TOENCODE_PATHS)?;
        let mut rows = stmt.query(())?;
        let mut files: Vec<PathBuf> = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let path: String = row.get(0)?;
            files.push(PathBuf::from(path));
        }
        Ok(files)
    }

    fn get_toencode_number(&self) -> Result<u64, rusqlite::Error> {
        self.query_one(TOENCODE_NUMBER, (), |row| {
            let num: u64 = row.get(0)?;
            Ok(num)
        })
    }

    fn get_modtime(&self, file: &Path) -> Result<u64> {
        Ok(
            self.query_one(GET_MODTIME, params![file.to_str().unwrap()], |row| {
                let modtime: u64 = row.get(0)?;
                Ok(modtime)
            })?,
        )
    }

    fn vacuum(&self) -> Result<()> {
        self.execute("VACUUM", ())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn check_localfiles() {
        let dbname = PathBuf::from("temp1.db");
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let mut counter = 0;
        let conn = Connection::new(Some(&dbname)).unwrap();
        for file in filenames {
            let filename = PathBuf::from(file);
            conn.insert_file(&filename).unwrap();
        }
        let mut stmt = conn.prepare(TOENCODE_PATHS).unwrap();
        let mut returned = stmt.query(()).unwrap();

        while let Ok(Some(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }

    #[test]
    fn check_update() {
        let dbname = PathBuf::from("temp2.db");
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let conn = Connection::new(Some(&dbname)).unwrap();
        for file in filenames {
            conn.insert_file(&Path::new(file).canonicalize().unwrap())
                .unwrap();
        }

        conn.execute(
            UPDATE_ITEM,
            params![
                Path::new("./samples/16bit.flac")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                true,
                ""
            ],
        )
        .unwrap();

        conn.update_file(&Path::new("./samples/16bit.flac").canonicalize().unwrap())
            .unwrap();

        let mut stmt = conn.prepare(TOENCODE_PATHS).unwrap();
        let mut returned = stmt.query(()).unwrap();
        let mut counter = 0;
        while let Ok(Some(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }
}
