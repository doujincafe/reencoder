use anyhow::{Result, anyhow};
use directories::BaseDirs;
use libsql::{Builder, Connection, params};
use smol::stream::Stream;
use std::{
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use crate::flac::{CURRENT_VENDOR, get_vendor};

const TABLE_CREATE: &str = "CREATE TABLE IF NOT EXISTS flacs (path TEXT PRIMARY KEY, toencode BOOLEAN NOT NULL, modtime INTEGER)";
const ADD_NEW_ITEM: &str = "INSERT INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const REPLACE_ITEM: &str = "REPLACE INTO flacs (path, toencode, modtime) VALUES (?1, ?2, ?3)";
const TOENCODE_QUERY: &str = "SELECT path FROM flacs WHERE toencode";
const TOENCODE_NUMBER: &str = "SELECT COUNT(*) from flacs WHERE toencode";
const CHECK_FILE: &str = "SELECT exists(SELECT 1 FROM flacs WHERE path = ?1)";
const FETCH_MODTIME: &str = "SELECT modtime FROM flacs WHERE path = ?1";
const FETCH_FILES: &str = "SELECT path FROM flacs";
const REMOVE_FILE: &str = "DELETE FROM flacs WHERE path = ?1";
const DEDUPE_DB: &str =
    "DELETE FROM flacs WHERE rowid NOT IN (SELECT MAX(rowid) FROM flacs GROUP BY path)";

#[derive(Debug, Clone)]
pub struct Database(Connection);

impl Database {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Builder::new_local(path).build().await?.connect()?;
        conn.execute(TABLE_CREATE, ()).await?;

        Ok(Database(conn))
    }

    pub async fn insert_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        let toencode = !matches!(get_vendor(&filename)?.as_str(), CURRENT_VENDOR);

        let modtime = filename
            .as_ref()
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.0
            .execute(
                ADD_NEW_ITEM,
                params![filename.as_ref().to_str().unwrap(), toencode, modtime],
            )
            .await?;

        Ok(())
    }

    pub async fn update_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        let modtime = filename
            .as_ref()
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        self.0
            .execute(
                REPLACE_ITEM,
                params![filename.as_ref().to_str().unwrap(), false, modtime],
            )
            .await?;

        Ok(())
    }

    pub async fn check_file(&self, filename: impl AsRef<Path>) -> Result<bool> {
        if let Some(row) = self
            .0
            .query(CHECK_FILE, params!(filename.as_ref().to_str().unwrap()))
            .await?
            .next()
            .await?
        {
            Ok(matches!(row.get_value(0)?, libsql::Value::Integer(1)))
        } else {
            Err(anyhow!("database error"))
        }
    }

    pub async fn get_modtime(&self, filename: impl AsRef<Path>) -> Result<u64> {
        if let Some(row) = self
            .0
            .query(FETCH_MODTIME, params!(filename.as_ref().to_str().unwrap()))
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

    pub async fn init_clean_files(
        &self,
    ) -> Result<impl Stream<Item = libsql::Result<libsql::Row>>> {
        self.0.execute(DEDUPE_DB, ()).await?;
        Ok(self.0.query(FETCH_FILES, ()).await?.into_stream())
    }

    pub async fn remove_file(&self, filename: impl AsRef<Path>) -> Result<()> {
        self.0
            .execute(REMOVE_FILE, params!(filename.as_ref().to_str().unwrap()))
            .await?;
        Ok(())
    }

    pub async fn get_toencode_stream(
        &self,
    ) -> Result<impl Stream<Item = libsql::Result<libsql::Row>>> {
        Ok(self.0.query(TOENCODE_QUERY, ()).await?.into_stream())
    }

    pub async fn get_toencode_number(&self) -> Result<u64> {
        Ok(self
            .0
            .query(TOENCODE_NUMBER, ())
            .await?
            .next()
            .await?
            .unwrap()
            .get::<u64>(0)?)
    }

    pub async fn vaccum(&self) -> Result<()> {
        self.0.execute("VACUUM", ()).await?;
        Ok(())
    }
}

pub async fn open_default_db() -> Result<Database> {
    if let Some(base_dir) = BaseDirs::new() {
        let db_name = Path::new(base_dir.data_dir()).join("reencoder.db");
        Ok(Database::new(db_name).await?)
    } else {
        Err(anyhow!("Failed to locate data directory"))
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use macro_rules_attribute::apply;
    use smol_macros::{Executor, test};

    use super::*;

    #[apply(test!)]
    async fn check_localfiles(ex: &Executor<'_>) {
        ex.spawn(async {
            let dbname = String::from("temp1.db");
            let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
            let mut counter = 0;
            let conn = Database::new(&dbname).await.unwrap();
            for file in filenames {
                let _ = conn.insert_file(&file.to_string()).await;
            }
            let returned = conn
                .0
                .query(TOENCODE_QUERY, ())
                .await
                .unwrap()
                .into_stream();
            pin_utils::pin_mut!(returned);

            while let Some(Ok(_)) = returned.next().await {
                counter += 1
            }
            std::fs::remove_file(dbname).unwrap();
            assert!(counter == 0)
        })
        .await;
    }

    #[apply(test!)]
    async fn check_update(ex: &Executor<'_>) {
        ex.spawn(async {
            let dbname = String::from("temp2.db");
            let filenames = ["16bit.flac", "24bit.flac", "32bit.flac"];
            let conn = Database::new(&dbname).await.unwrap();
            for file in filenames {
                let _ = conn
                    .insert_file(Path::new(file).canonicalize().unwrap())
                    .await;
            }

            let _ = conn
                .0
                .execute(
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
                )
                .await;

            conn.update_file(
                Path::new("16bit.flac")
                    .canonicalize()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            )
            .await
            .unwrap();

            let returned = conn
                .0
                .query(TOENCODE_QUERY, ())
                .await
                .unwrap()
                .into_stream();
            pin_utils::pin_mut!(returned);
            let mut counter = 0;
            while let Some(Ok(_)) = returned.next().await {
                counter += 1
            }
            std::fs::remove_file(dbname).unwrap();
            assert!(counter == 0)
        })
        .await;
    }
}
