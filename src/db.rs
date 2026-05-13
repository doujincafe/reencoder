use anyhow::{Result, anyhow};
use directories::BaseDirs;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use stoolap::{Database, named_params};

use crate::flac::{CURRENT_VENDOR, get_vendor};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY UNIQUE, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_ITEM: &str =
    "INSERT INTO flacs (path, toencode, modtime) VALUES (:path, :toencode, :modtime)";
const UPDATE_ITEM: &str =
    "UPDATE flacs SET toencode = :toencode, modtime = :modtime WHERE path = :path";
const TOENCODE_PATHS: &str = "SELECT path FROM flacs WHERE toencode";
const TOENCODE_NUMBER: &str = "SELECT COUNT(*) from flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = :path)";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flacs WHERE path = :path";
const GET_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = :path";

pub(crate) fn init_connection(path: Option<&PathBuf>) -> Result<Database> {
    let db = if let Some(file) = path {
        Database::open(file.to_str()?)?
    } else if let Some(base_dir) = BaseDirs::new() {
        let file = Path::new(base_dir.data_dir()).join("reencoder.db");
        Database::open(file.to_str()?)?
    } else {
        return Err(anyhow!("Failed to locate data directory"));
    };
    db.execute(TABLE_CREATE, ())?;
    Ok(conn)
}

pub(crate) fn insert_file(conn: &Database, filename: &Path) -> Result<()> {
    let toencode = !matches!(get_vendor(filename)?.as_str(), CURRENT_VENDOR);

    let modtime = filename
        .metadata()?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    conn.execute_named(
        ADD_ITEM,
        named_params! {filename: filename.to_str().unwrap(), toencode: toencode, modtime: modtime},
    )?;

    Ok(())
}

pub(crate) fn update_file(conn: &Database, filename: &Path) -> Result<()> {
    let modtime = filename
        .metadata()?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    conn.execute_named(
        UPDATE_ITEM,
        named_params! {filename: filename.to_str().unwrap(), toencode: false, modtime: modtime},
    )?;

    Ok(())
}

pub(crate) fn check_file(conn: &Database, filename: &Path) -> Result<bool> {
    let tocheck: bool =
        conn.query_one_named(CHECK_FILE, named_params! {filename: filename.to_str()?})?;
    Ok(tocheck)
}

pub(crate) fn init_clean_files(conn: &Database) -> Result<Vec<PathBuf>, rusqlite::Error> {
    let mut files: Vec<PathBuf> = Vec::new();
    for row in conn.query(FETCH_FILES, ())? {
        let path: String = row.get(0)?;
        files.push(PathBuf::from(path));
    }
    Ok(files)
}

pub(crate) fn remove_file(conn: &Database, filename: &Path) -> Result<()> {
    conn.execute_named(REMOVE_FILE, named_params! {filename: filename.to_str()?})?;
    Ok(())
}

pub(crate) fn get_toencode_files(conn: &Database) -> Result<Vec<PathBuf>, rusqlite::Error> {
    let mut files: Vec<PathBuf> = Vec::new();
    for row in conn.query(TOENCODE_PATHS, ())? {
        let path: String = row.get(0)?;
        files.push(PathBuf::from(path));
    }
    Ok(files)
}

pub(crate) fn get_toencode_number(conn: &Database) -> Result<u64, stoolap::Error> {
    let num: u64 = conn.query_one(TOENCODE_NUMBER, ())?;
    Ok(num)
}

pub(crate) fn get_modtime(conn: &Database, file: &Path) -> Result<u64> {
    let modtime: u64 = conn.query_one_named(GET_MODTIME, named_params! {path: file.to_str()?})?;
    Ok(modtime)
}

pub(crate) fn vacuum(conn: &Database) -> Result<()> {
    conn.execute("VACUUM", ())?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use rand::Rng;

    fn generate_random_string() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();

        (0..16)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    #[test]
    fn check_localfiles() {
        let dbname = PathBuf::from(format!(generate_random_string(), ".db"));
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let mut counter = 0;
        let conn = init_connection(Some(&dbname)).unwrap();
        for file in filenames {
            let filename = PathBuf::from(file);
            insert_file(&conn, &filename).unwrap();
        }
        let mut returned = conn.query(TOENCODE_PATHS, ()).unwrap();

        while let Some(Ok(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }

    #[test]
    fn check_update() {
        let dbname = PathBuf::from(format!(generate_random_string(), ".db"));
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let conn = init_connection(Some(&dbname)).unwrap();
        for file in filenames {
            insert_file(&conn, &Path::new(file).canonicalize().unwrap()).unwrap();
        }

        conn.execute_named(
            UPDATE_ITEM,
            named_params! {
                path: Path::new("./samples/16bit.flac")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                toencode: true,
                modtime: ""
            },
        )
        .unwrap();

        update_file(
            &conn,
            &Path::new("./samples/16bit.flac").canonicalize().unwrap(),
        )
        .unwrap();

        let mut returned = conn.query(TOENCODE_PATHS, ()).unwrap();
        let mut counter = 0;
        while let Some(Ok(_)) = returned.next() {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }
}
