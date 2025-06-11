use anyhow::{Ok, Result, anyhow};
use directories::BaseDirs;
use futures_util::StreamExt;
use libsql::{Builder, Connection, params};
use std::{
    ffi::OsStr,
    fmt::Display,
    path::{Path, absolute},
};

use crate::flac;

pub async fn open_db() -> Result<Connection> {
    let conn = if let Some(base_dir) = BaseDirs::new() {
        let db_name = Path::new(base_dir.data_dir()).join("reencoder.db");
        Builder::new_local(db_name).build().await?.connect()?
    } else {
        return Err(anyhow!("Failed to locate data directory"));
    };

    conn.execute("CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY, vendor TEXT, toencode BOOLEAN NOT NULL)", ()).await?;

    Ok(conn)
}

#[derive(Debug)]
enum Errors {
    EmptyQuery,
}

impl Display for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Errors::EmptyQuery => write!(f, "Empty query"),
        }
    }
}

trait Reencoder {
    async fn insert_file(&self, filename: &impl AsRef<OsStr>) -> Result<()>;
    async fn update_file(&self, filename: &impl AsRef<OsStr>) -> Result<()>;
    async fn get_files_toencode(&self) -> Result<Vec<String>>;
}

impl Reencoder for Connection {
    async fn insert_file(&self, filename: &impl AsRef<OsStr>) -> Result<()> {
        let file = Path::new(filename);
        let vendor = flac::get_vendor(file);
        let toencode = !matches!(vendor.as_str(), flac::CURRENT_VENDOR);

        let abs_filename = absolute(file)?;

        self.execute(
            "INSERT INTO flacs (path, vendor, toencode) VALUES (?1, ?2, ?3)",
            params![abs_filename.to_str().unwrap(), vendor, toencode],
        )
        .await?;

        Ok(())
    }

    async fn update_file(&self, filename: &impl AsRef<OsStr>) -> Result<()> {
        let abs_filename = absolute(Path::new(filename))?;

        self.execute(
            "REPLACE INTO flacs (path, vendor, toencode) VALUES (?1, ?2, ?3)",
            params![abs_filename.to_str().unwrap(), flac::CURRENT_VENDOR, false],
        )
        .await?;

        Ok(())
    }

    async fn get_files_toencode(&self) -> Result<Vec<String>> {
        let rows = self
            .query("SELECT path FROM flacs WHERE toencode", ())
            .await?;
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
        conn.execute("CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY, vendor TEXT, toencode BOOLEAN NOT NULL)", ()).await.unwrap();
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
                "REPLACE INTO flacs (path, vendor, toencode) VALUES (?1, ?2, ?3)",
                params![
                    absolute(Path::new("16bit.flac")).unwrap().to_str(),
                    "",
                    true
                ],
            )
            .await;

        conn.update_file(&"16bit.flac".to_string()).await.unwrap();

        let returned = conn.get_files_toencode().await.unwrap();
        std::fs::remove_file(dbname).unwrap();
        assert!(returned.is_empty())
    }
}
