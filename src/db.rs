use crate::flac::{CURRENT_VENDOR, get_vendor};
use anyhow::{Result, anyhow};
use directories::BaseDirs;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio::fs;
use turso::{Connection, params, transaction::Transaction};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY UNIQUE, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_FILE: &str = "INSERT INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const UPDATE_FILE: &str = "UPDATE flacs SET toencode = ?2, modtime = ?3 WHERE path = ?1";
const TOENCODE_PATHS: &str = "SELECT path FROM flacs WHERE toencode";
const TOENCODE_NUMBER: &str = "SELECT COUNT(*) from flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = ?1)";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flacs WHERE path = ?1";
const GET_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = ?1";

pub(crate) async fn init_db(path: Option<&PathBuf>) -> Result<turso::Database> {
    let db = if let Some(file) = path {
        turso::Builder::new_local(file.canonicalize()?.to_str().unwrap())
            .build()
            .await?
    } else if let Some(base_dir) = BaseDirs::new() {
        let file = base_dir.data_dir().join("reencoder.db");
        turso::Builder::new_local(file.to_str().unwrap())
            .build()
            .await?
    } else {
        return Err(anyhow!("Failed to locate data directory"));
    };
    let conn = db.connect()?;
    conn.execute(TABLE_CREATE, ()).await?;
    Ok(db)
}

pub(crate) async fn insert_file<'a>(tx: Transaction<'a>, filename: &Path) -> Result<()> {
    let toencode = !matches!(get_vendor(filename)?.as_str(), CURRENT_VENDOR);

    let modtime = fs::metadata(filename)
        .await?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    tx.execute(
        ADD_FILE,
        params![filename.to_str().unwrap(), toencode, modtime],
    )
    .await?;

    tx.commit().await?;

    Ok(())
}

pub(crate) async fn update_file<'a>(tx: Transaction<'a>, filename: &Path) -> Result<()> {
    let modtime = fs::metadata(filename)
        .await?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    tx.execute(
        UPDATE_FILE,
        params![filename.to_str().unwrap(), false, modtime],
    )
    .await?;

    tx.commit().await?;

    Ok(())
}

pub(crate) async fn check_file<'a>(tx: &Transaction<'a>, filename: &Path) -> Result<bool> {
    Ok(tx
        .query(CHECK_FILE, params!(filename.to_str().unwrap()))
        .await?
        .next()
        .await?
        .unwrap()
        .get::<bool>(0)?)
}

pub(crate) async fn fetch_files(conn: &Connection) -> Result<Vec<PathBuf>, turso::Error> {
    let mut rows = conn.query(FETCH_FILES, ()).await?;
    let mut files = Vec::new();
    while let Ok(Some(row)) = rows.next().await {
        let path: String = row.get(0)?;
        files.push(PathBuf::from(path));
    }

    Ok(files)
}

pub(crate) async fn remove_file<'a>(tx: Transaction<'a>, filename: &Path) -> Result<()> {
    tx.execute(REMOVE_FILE, params!(filename.to_str().unwrap()))
        .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn get_toencode_files(conn: &Connection) -> Result<Vec<PathBuf>, turso::Error> {
    let mut rows = conn.query(TOENCODE_PATHS, ()).await?;
    let mut files: Vec<PathBuf> = Vec::new();
    while let Ok(Some(row)) = rows.next().await {
        let path: String = row.get(0)?;
        files.push(PathBuf::from(path));
    }

    Ok(files)
}

pub(crate) async fn get_toencode_number(conn: &Connection) -> Result<u64, turso::Error> {
    conn.query(TOENCODE_NUMBER, ())
        .await?
        .next()
        .await?
        .unwrap()
        .get::<u64>(0)
}

pub(crate) async fn get_modtime<'a>(tx: &Transaction<'a>, file: &Path) -> Result<u64> {
    Ok(tx
        .query(GET_MODTIME, params![file.to_str().unwrap()])
        .await?
        .next()
        .await?
        .unwrap()
        .get::<u64>(0)?)
}

pub(crate) async fn vacuum<'a>(tx: Transaction<'a>) -> Result<()> {
    tx.execute("VACUUM", ()).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use turso::transaction::{Transaction, TransactionBehavior::Deferred};

    #[tokio::test]
    async fn check_localfiles() {
        let dbname = PathBuf::from("temp1.db");
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let mut counter = 0;
        let db = init_db(Some(&dbname)).await.unwrap();
        let mut conn = db.connect().unwrap();
        for file in filenames {
            let path = PathBuf::from(file);
            let tx = Transaction::new(&mut conn, Deferred).await.unwrap();
            insert_file(tx, &path).await.unwrap();
        }
        let mut returned = conn.query(TOENCODE_PATHS, ()).await.unwrap();
        while let Ok(Some(_)) = returned.next().await {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }

    #[tokio::test]
    async fn check_update() {
        let dbname = PathBuf::from("temp2.db");
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
        ];
        let db = init_db(Some(&dbname)).await.unwrap();
        let mut conn = db.connect().unwrap();
        for file in filenames {
            let tx = Transaction::new(&mut conn, Deferred).await.unwrap();
            insert_file(tx, &Path::new(file).canonicalize().unwrap())
                .await
                .unwrap();
        }
        conn.execute(
            UPDATE_FILE,
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
        .await
        .unwrap();

        let tx = Transaction::new(&mut conn, Deferred).await.unwrap();

        update_file(
            tx,
            &Path::new("./samples/16bit.flac").canonicalize().unwrap(),
        )
        .await
        .unwrap();

        let mut returned = conn.query(TOENCODE_PATHS, ()).await.unwrap();
        let mut counter = 0;
        while let Ok(Some(_)) = returned.next().await {
            counter += 1
        }
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 0)
    }
}
