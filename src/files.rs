use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use pin_utils::pin_mut;
use std::{
    fmt::Display,
    path::{Path, PathBuf, absolute},
    time::UNIX_EPOCH,
};
use tokio::{fs::read_dir, task::JoinSet};

use crate::{db::Database, flac::handle_encode};

#[derive(Debug)]
pub struct FileError {
    file: PathBuf,
    error: anyhow::Error,
}

impl FileError {
    pub fn new(file: PathBuf, error: anyhow::Error) -> Self {
        FileError { file, error }
    }
}

impl Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error: {}\t on file {}",
            self.error,
            self.file.to_string_lossy()
        )
    }
}

async fn handle_file(file: PathBuf, conn: Database) -> Result<()> {
    match conn.check_file(&file).await {
        Ok(true) => {
            let modtime = file
                .metadata()?
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_secs();
            let db_time = conn.get_modtime(&file).await?;
            if modtime != db_time {
                if let Err(error) = conn.update_file(&file).await {
                    return Err(anyhow!(FileError::new(file, error)));
                };
            }
            return Ok(());
        }
        Err(error) => return Err(anyhow!(FileError::new(file, error))),
        _ => {}
    }

    if let Err(error) = conn.insert_file(&file).await {
        return Err(anyhow!(FileError::new(file, error)));
    }

    Ok(())
}

pub async fn index_files_recursively(path: &Path, conn: &Database) -> Result<()> {
    if !path.is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = absolute(path)?;
    let mut tasks = JoinSet::new();

    let mut dirs = vec![abspath];

    while let Some(dir) = dirs.pop() {
        let mut read_dir = read_dir(dir).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "flac" {
                        let newconn = conn.clone();
                        tasks.spawn(async move { handle_file(path, newconn).await });
                    }
                }
            }
        }
    }

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Err(error)) => eprintln!("{error}"),
            Err(error) => eprintln!("Error encountered:\t{}", error),
            _ => {}
        }
    }

    Ok(())
}

pub async fn reencode_files(conn: &Database) -> Result<()> {
    let stream = conn.get_toencode_stream().await?;
    pin_mut!(stream);

    let mut tasks = JoinSet::new();

    while let Some(Ok(row)) = stream.next().await {
        if let Some(file) = row.get_value(0)?.as_text() {
            let filename = PathBuf::from(file.clone());
            tasks.spawn_blocking(move || handle_encode(filename));
        }
    }

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Err(error)) => eprintln!("{error}"),
            Err(error) => eprintln!("Error encountered:\t{}", error),
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lots_of_files() {
        let conn = Database::new("temp3.db").await.unwrap();
        index_files_recursively(Path::new("/mnt/Music"), &conn)
            .await
            .unwrap();
        println!(
            "\n{}",
            conn.0
                .query("SELECT COUNT(DISTINCT path) FROM flacs", ())
                .await
                .unwrap()
                .next()
                .await
                .unwrap()
                .unwrap()
                .get_value(0)
                .unwrap()
                .as_integer()
                .unwrap()
        );
    }
}
