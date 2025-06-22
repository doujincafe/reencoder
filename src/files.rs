use anyhow::{Result, anyhow};
use futures_util::StreamExt;
#[cfg(not(test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use pin_utils::pin_mut;
use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use walkdir::WalkDir;

use crate::{db::Database, flac::encode_file};

#[cfg(not(test))]
const BAR_TEMPLATE: &str = "{msg:<} [{wide_bar:.green/cyan}] Elapsed: {elapsed} {pos:>7}/{len:7}";
#[cfg(not(test))]
const SPINNER_TEMPLATE: &str = "Removed from db: {pos:.green}";

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
            "error: {}\ton file {}",
            self.error,
            self.file.to_string_lossy()
        )
    }
}

impl Error for FileError {}

async fn handle_file(file: impl AsRef<Path>, conn: Database) -> Result<()> {
    match conn.check_file(&file).await {
        Ok(true) => {
            let modtime = file
                .as_ref()
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

pub async fn index_files_recursively(
    path: impl AsRef<Path>,
    conn: &Database,
    canceltoken: CancellationToken,
) -> Result<()> {
    if !path.as_ref().is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = path.as_ref().canonicalize()?;

    let mut tasks: JoinSet<Result<(), anyhow::Error>> = JoinSet::new();
    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
        .with_message("Indexing");

    for entry in WalkDir::new(abspath) {
        let path = entry?.into_path();
        if !path.is_file() {
            continue;
        }

        if path.extension().is_some_and(|x| x == "flac") {
            let newconn = conn.clone();
            let newtoken = canceltoken.clone();
            #[cfg(not(test))]
            let newbar = bar.clone();

            tasks.spawn(async move {
                tokio::select! {
                    _ = newtoken.cancelled() => Ok(()),
                    res = async {
                        handle_file(path, newconn).await?;
                        #[cfg(not(test))]
                        newbar.inc(1);
                        Ok(())
                    } => res
                }
            });

            #[cfg(not(test))]
            bar.inc_length(1);
        }
    }

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Err(error)) => eprintln!("{error}"),
            Err(error) => eprintln!("Error encountered:\t{}", error),
            _ => {}
        }
    }
    #[cfg(not(test))]
    {
        if canceltoken.is_cancelled() {
            bar.abandon_with_message("Indexing aborted");
        } else {
            bar.finish_with_message("Finished indexing");
        }
    }
    Ok(())
}

pub async fn reencode_files(conn: &Database, canceltoken: CancellationToken) -> Result<()> {
    let stream = conn.get_toencode_stream().await?;
    pin_mut!(stream);

    let mut tasks = JoinSet::new();

    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(
        Some(conn.get_toencode_number().await?),
        ProgressDrawTarget::stdout_with_hz(60),
    )
    .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
    .with_message("Reencoding");

    while let Some(Ok(row)) = stream.next().await {
        let filename = Path::new(row.get_str(0)?).canonicalize()?;
        if filename.exists() {
            let newconn = conn.clone();
            let newtoken = canceltoken.clone();
            #[cfg(not(test))]
            let newbar = bar.clone();
            tasks.spawn(async move {
                let file = filename.clone();
                tokio::select! {
                    _ = newtoken.cancelled() => {
                        let _ = std::fs::remove_file(filename.with_extension("tmp.metadata_edit"));
                        let _ = std::fs::remove_file(filename.with_extension("tmp"));
                        Ok(())
                    }
                    res = async {
                        if let Err(error) = tokio::task::spawn_blocking(move || encode_file(file)).await? {
                            let _ = std::fs::remove_file(filename.with_extension("tmp.metadata_edit"));
                            let _ = std::fs::remove_file(filename.with_extension("tmp"));
                            return Err(anyhow!(FileError::new(&filename, error)));
                        };
                        if let Err(error) = newconn.update_file(&filename).await {
                            return Err(anyhow!(FileError::new(&filename, error)));
                        };
                        #[cfg(not(test))]
                        newbar.inc(1);
                        Ok(())
                    } => res
                }
            });
        }
    }

    while let Some(task) = tasks.join_next().await {
        match task {
            Ok(Err(error)) => eprintln!("Error encountered:\t{error}"),
            Err(error) => eprintln!("Error encountered:\t{error}"),
            _ => {}
        }
    }

    #[cfg(not(test))]
    {
        if canceltoken.is_cancelled() {
            bar.abandon_with_message("Reencoding aborted");
        } else {
            bar.finish_with_message("Finished encoding");
        }
    }

    Ok(())
}

pub async fn clean_files(conn: &Database) -> Result<()> {
    let mut tasks: JoinSet<std::result::Result<(), anyhow::Error>> = JoinSet::new();

    let query_res = conn.init_clean_files().await?;
    pin_mut!(query_res);
    #[cfg(not(test))]
    let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?);

    while let Some(Ok(row)) = query_res.next().await {
        let path = PathBuf::from(row.get_str(0)?);
        let newconn = conn.clone();
        #[cfg(not(test))]
        let newspinner = spinner.clone();
        tasks.spawn(async move {
            if !path.exists() {
                newconn.remove_file(path).await?;
                #[cfg(not(test))]
                newspinner.inc(1);
            }
            Ok(())
        });
    }

    tasks.join_all().await;
    #[cfg(not(test))]
    spinner.finish();

    conn.vaccum().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_index_lots_of_files() {
        let conn = Database::new("temp3.db").await.unwrap();
        let token = CancellationToken::new();
        index_files_recursively(Path::new("./testfiles"), &conn, token)
            .await
            .unwrap();

        std::fs::remove_file("temp3.db").unwrap();
    }

    #[test]
    fn test_reencode_lots_of_files() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .max_blocking_threads(4)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let conn = Database::new("temp4.db").await.unwrap();
            let token = CancellationToken::new();
            index_files_recursively(Path::new("./testfiles"), &conn, token)
                .await
                .unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
            let token = CancellationToken::new();
            reencode_files(&conn, token).await.unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
        });

        std::fs::remove_file("temp4.db").unwrap();
    }
}
