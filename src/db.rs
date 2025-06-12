use anyhow::{Result, anyhow};
use directories::BaseDirs;
use futures_util::StreamExt;
use libsql::{Builder, Connection, params};
use std::{
    ffi::OsStr,
    fmt::Display,
    path::{Path, absolute},
    time::{Duration, UNIX_EPOCH},
};

use crate::flac::{CURRENT_VENDOR, get_vendor};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_NEW_ITEM: &str = "INSERT INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const REPLACE_ITEM: &str = "REPLACE INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const TOENCODE_QUERY: &str = "SELECT path FROM flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = ?1)";
const FETCH_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = ?1";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flac WHERE path = ?1";

#[derive(Debug)]
pub enum Errors {
    EmptyQuery,
}

impl Display for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Errors::EmptyQuery => write!(f, "Empty query"),
        }
    }
}

pub trait Reencoder {
    async fn insert_file(&self, filename: &impl AsRef<OsStr>) -> Result<()>;
    async fn update_file(&self, filename: &impl AsRef<OsStr>) -> Result<()>;
    async fn get_files_toencode(&self) -> Result<Vec<String>>;
    async fn check_file(&self, filename: &impl AsRef<OsStr>) -> Result<bool>;
    async fn get_modtime(&self, filename: &impl AsRef<OsStr>) -> Result<u64>;
    async fn clean_files(&self) -> Result<()>;
}

impl Reencoder for Connection {
    async fn insert_file(&self, filename: &impl AsRef<OsStr>) -> Result<()> {
        let abs_filename = absolute(Path::new(filename))?;
        let toencode = !matches!(get_vendor(&abs_filename)?.as_str(), CURRENT_VENDOR);

        let modtime = abs_filename
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.execute(
            ADD_NEW_ITEM,
            params![abs_filename.to_str().unwrap(), toencode, modtime],
        )
        .await?;

        Ok(())
    }

    async fn update_file(&self, filename: &impl AsRef<OsStr>) -> Result<()> {
        let abs_filename = absolute(Path::new(filename))?;

        let modtime = abs_filename
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.execute(
            REPLACE_ITEM,
            params![abs_filename.to_str().unwrap(), false, modtime],
        )
        .await?;

        Ok(())
    }

    async fn get_files_toencode(&self) -> Result<Vec<String>> {
        let rows = self.query(TOENCODE_QUERY, ()).await?;
        if rows.column_count() == 0 {
            return Err(anyhow!(Errors::EmptyQuery));
        };

        let filenames = rows
            .into_stream()
            .map(|row| row.unwrap().get_str(0).unwrap().to_string())
            .collect::<Vec<String>>()
            .await;

        Ok(filenames)
    }

    async fn check_file(&self, filename: &impl AsRef<OsStr>) -> Result<bool> {
        let abs_filename = absolute(Path::new(filename))?;

        if let Some(row) = self
            .query(CHECK_FILE, params!(abs_filename.to_str().unwrap()))
            .await?
            .next()
            .await?
        {
            Ok(matches!(row.get_value(0)?, libsql::Value::Integer(1)))
        } else {
            Err(anyhow!("database error"))
        }
    }

    async fn get_modtime(&self, filename: &impl AsRef<OsStr>) -> Result<u64> {
        let abs_filename = absolute(Path::new(filename))?;

        if let Some(row) = self
            .query(FETCH_MODTIME, params!(abs_filename.to_str().unwrap()))
            .await?
            .next()
            .await?
        {
            if let Some(sec) = row.get_value(0)?.as_integer() {
                Ok(Duration::from_secs(*sec as u64).as_secs())
            } else {
                Ok(Duration::from_secs(0).as_secs())
            }
        } else {
            Err(anyhow!("database error"))
        }
    }

    async fn clean_files(&self) -> Result<()> {
        let mut tasks = tokio::task::JoinSet::new();
        while let Ok(Some(row)) = self.query(FETCH_FILES, ()).await?.next().await {
            let path = absolute(Path::new(row.get_str(0)?))?;
            let conn = self.clone();
            tasks.spawn(async move {
                if !path.exists() {
                    let _ = conn
                        .execute(REMOVE_FILE, params!(path.to_str().unwrap()))
                        .await;
                }
            });
        }

        tasks.join_all().await;

        Ok(())
    }
}

pub async fn open_db() -> Result<Connection> {
    let conn = if let Some(base_dir) = BaseDirs::new() {
        let db_name = Path::new(base_dir.data_dir()).join("reencoder.db");
        Builder::new_local(db_name).build().await?.connect()?
    } else {
        return Err(anyhow!("Failed to locate data directory"));
    };

    conn.execute(TABLE_CREATE, ()).await?;

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn dummy_db(name: impl AsRef<Path>) -> Connection {
        let conn = Builder::new_local(name)
            .build()
            .await
            .unwrap()
            .connect()
            .unwrap();
        conn.execute(TABLE_CREATE, ()).await.unwrap();
        conn
    }

    #[tokio::test]
    async fn check_localfiles() {
        let dbname = String::from("temp1.db");
        let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
        let conn = dummy_db(&dbname).await;
        for file in filenames {
            let _ = conn.insert_file(&file.to_string()).await;
        }
        let returned = conn.get_files_toencode().await.unwrap();
        std::fs::remove_file(dbname).unwrap();
        assert!(returned.is_empty())
    }

    #[tokio::test]
    async fn check_update() {
        let dbname = String::from("temp2.db");
        let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
        let conn = dummy_db(&dbname).await;
        for file in filenames {
            let _ = conn.insert_file(&file.to_string()).await;
        }

        let _ = conn
            .execute(
                REPLACE_ITEM,
                params![
                    absolute(Path::new("16bit.flac")).unwrap().to_str(),
                    true,
                    ""
                ],
            )
            .await;

        conn.update_file(&"16bit.flac".to_string()).await.unwrap();

        let returned = conn.get_files_toencode().await.unwrap();
        std::fs::remove_file(dbname).unwrap();
        assert!(returned.is_empty())
    }
}
