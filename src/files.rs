use anyhow::{Result, anyhow};
use futures_util::StreamExt;
#[cfg(not(test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use pin_utils::pin_mut;
use rayon::prelude::*;
use smol::{
    Executor,
    fs::{File, metadata},
};
use std::time;
use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, UNIX_EPOCH},
};
use walkdir::WalkDir;

use crate::{db::Database, flac::handle_encode};

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
            let modtime = metadata(file.as_ref())
                .await?
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

pub fn index_files_recursively(
    path: impl AsRef<Path>,
    conn: &Database,
    running: Arc<AtomicBool>,
) -> Result<()> {
    if !path.as_ref().is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = path.as_ref().canonicalize()?;

    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
        .with_message("Indexing");

    let (tx, rx) = mpsc::channel();
    let ex = Executor::new();

    for entry in WalkDir::new(abspath) {
        if running.load(Ordering::SeqCst) {
            let path = entry.unwrap().into_path();
            if !path.is_file() {
                continue;
            }
            if path.extension().is_some_and(|x| x == "flac") {
                let newconn = conn.clone();
                let newrunning = running.clone();
                let newtx = tx.clone();
                #[cfg(not(test))]
                let newbar = bar.clone();

                ex.spawn(async move {
                    std::thread::sleep(Duration::from_secs(2));
                    println!("reached");
                    let _ = newtx.send(FileError::new(path, anyhow!("error!")));
                    #[cfg(not(test))]
                    newbar.inc(1);
                })
                .detach();

                /* ex.spawn(async move {
                    if !newrunning.load(Ordering::SeqCst) {
                        match handle_file(&path, newconn).await {
                            Err(error) => newtx.send(FileError::new(path, error)),
                            Ok(_) => {
                                #[cfg(not(test))]
                                newbar.inc(1);
                                Ok(())
                            }
                        }
                    } else {
                        Ok(())
                    }
                })
                .detach(); */

                #[cfg(not(test))]
                bar.inc_length(1);
            }
        }
    }

    drop(tx);

    while !ex.is_empty() {
        if let Ok(message) = rx.recv() {
            eprintln!("{}", message);
        }
    }

    #[cfg(not(test))]
    {
        if !running.load(Ordering::SeqCst) {
            bar.abandon_with_message("Indexing aborted");
        } else {
            bar.finish_with_message("Finished indexing");
        }
    }
    Ok(())
}

/* pub fn reencode_files(conn: &Database) -> Result<()> {
    let stream = conn.get_toencode_stream().await?;
    pin_mut!(stream);

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
                        if let Err(error) = tokio::task::spawn_blocking(move || handle_encode(file)).await? {
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

    Ok(())
}

pub fn clean_files(conn: &Database) -> Result<()> {
    let ex = Executor::new();

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
} */

#[cfg(test)]
mod tests {
    use super::*;
    use macro_rules_attribute::apply;
    use smol_macros::{Executor, test};

    #[apply(test!)]
    async fn test_index_lots_of_files(ex: &Executor<'_>) {
        ex.spawn(async {
            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();

            ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            })
            .unwrap();
            let conn = Database::new("temp3.db").await.unwrap();
            index_files_recursively(Path::new("./testfiles"), &conn, running).unwrap();
            std::fs::remove_file("temp3.db").unwrap();
        })
        .await;
    }

    /* #[apply(test!)]
    async fn test_reencode_lots_of_files(ex: &Executor<'_>) {
        ex.spawn(async {
            let conn = Database::new("temp4.db").await.unwrap();
            index_files_recursively(Path::new("./testfiles"), &conn).unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
            reencode_files(&conn).unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
            std::fs::remove_file("temp4.db").unwrap();
        })
        .await;
    } */
}
