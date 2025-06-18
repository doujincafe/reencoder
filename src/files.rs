use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use pin_utils::pin_mut;
use std::{
    fmt::Display,
    path::{Path, PathBuf},
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
    pub fn new(file: impl AsRef<Path>, error: anyhow::Error) -> Self {
        FileError {
            file: file.as_ref().to_path_buf(),
            error,
        }
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

pub async fn index_files_recursively(path: impl AsRef<Path>, conn: &Database) -> Result<()> {
    if !path.as_ref().is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = path.as_ref().canonicalize()?;
    let mut tasks = JoinSet::new();

    let mut dirs = vec![abspath];

    let mut counter: u64 = 0;

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
            _ => {
                counter += 1;
                print!("\rParsed files:\t{counter}");
            }
        }
    }
    Ok(())
}

fn check_path(folderpath: Option<&PathBuf>) -> (PathBuf, bool) {
    if let Some(real_path) = folderpath {
        (real_path.to_owned(), false)
    } else {
        (PathBuf::new(), true)
    }
}

pub async fn reencode_files(folderpath: Option<&PathBuf>, conn: &Database) -> Result<()> {
    let (path, nocheck) = check_path(folderpath);

    let stream = conn.get_toencode_stream().await?;
    pin_mut!(stream);

    let mut tasks = JoinSet::new();

    let mut counter: u64 = 0;

    while let Some(Ok(row)) = stream.next().await {
        if let Some(file) = row.get_value(0)?.as_text() {
            let filename = Path::new(file).canonicalize()?;
            if nocheck || filename.starts_with(&path) {
                tasks.spawn_blocking(move || handle_encode(filename));
            }
        }
    }

    let mut update_tasks = JoinSet::new();

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Ok(path)) => {
                let newconn = conn.clone();
                update_tasks.spawn(async move { newconn.update_file(path).await });
                counter += 1;
                print!("\rReencoded files:\t{counter}")
            }
            Ok(Err(error)) => eprintln!("{error}"),
            Err(error) => eprintln!("Error encountered:\t{}", error),
        }
    }

    while let Some(task) = update_tasks.join_next().await {
        match task {
            Ok(Err(error)) => eprintln!("{error}"),
            Err(error) => eprintln!("Error encountered:\t{}", error),
            _ => {}
        }
    }

    Ok(())
}

pub async fn count_reencode_files(folderpath: Option<&PathBuf>, conn: &Database) -> Result<u64> {
    let (path, nocheck) = check_path(folderpath);

    let mut counter: u64 = 0;
    let stream = conn.get_toencode_stream().await?;
    pin_mut!(stream);

    while let Some(Ok(row)) = stream.next().await {
        if let Some(file) = row.get_value(0)?.as_text() {
            let filename = Path::new(file).canonicalize()?;
            if nocheck || filename.starts_with(&path) {
                counter += 1;
            }
        }
    }

    Ok(counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_index_lots_of_files() {
        let conn = Database::new("temp3.db").await.unwrap();
        index_files_recursively(Path::new("./testfiles"), &conn)
            .await
            .unwrap();

        std::fs::remove_file("temp3.db").unwrap();
    }

    #[tokio::test]
    async fn test_reencode_lots_of_files() {
        let conn = Database::new("temp4.db").await.unwrap();
        let path = PathBuf::from("./testfiles");
        index_files_recursively(Path::new("./testfiles"), &conn)
            .await
            .unwrap();
        println!("\n{}", count_reencode_files(None, &conn).await.unwrap());
        reencode_files(Some(&path), &conn).await.unwrap();
        std::fs::remove_file("temp4.db").unwrap();
    }
}
