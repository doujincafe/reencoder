use anyhow::{Result, anyhow};
#[cfg(not(test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use rayon::prelude::*;
use smol::{Executor, fs::metadata};
use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::UNIX_EPOCH,
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
    handler: Arc<AtomicBool>,
) -> Result<()> {
    if !path.as_ref().is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = path.as_ref().canonicalize()?;

    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
        .with_message("Indexing");

    let ex = Executor::new();

    let mut tasks = Vec::new();

    for entry in WalkDir::new(abspath) {
        if handler.load(Ordering::SeqCst) {
            let path = entry.unwrap().into_path();
            if !path.is_file() {
                continue;
            }
            if path.extension().is_some_and(|x| x == "flac") {
                let newconn = conn.clone();
                let newhandler = handler.clone();
                #[cfg(not(test))]
                let newbar = bar.clone();

                tasks.push(ex.spawn(async move {
                    if newhandler.load(Ordering::SeqCst) {
                        match handle_file(&path, newconn).await {
                            Err(error) => Err(FileError::new(path, error)),
                            Ok(_) => {
                                #[cfg(not(test))]
                                newbar.inc(1);
                                Ok(())
                            }
                        }
                    } else {
                        Ok(())
                    }
                }));

                #[cfg(not(test))]
                bar.inc_length(1);
            }
        } else {
            break;
        }
    }

    tasks.par_iter_mut().for_each(|task| {
        if handler.load(Ordering::SeqCst) {
            if let Err(error) = smol::block_on(async { ex.run(task).await }) {
                eprintln!("{error}")
            }
        }
    });

    #[cfg(not(test))]
    {
        if handler.load(Ordering::SeqCst) {
            bar.finish_with_message("Finished indexing");
        } else {
            bar.abandon_with_message("Indexing aborted");
        }
    }
    Ok(())
}

pub fn reencode_files(conn: &Database, handler: Arc<AtomicBool>) -> Result<()> {
    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(
        Some(smol::block_on(async { conn.get_toencode_number().await })?.try_into()?),
        ProgressDrawTarget::stdout_with_hz(60),
    )
    .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
    .with_message("Reencoding");

    let files = smol::block_on(async { conn.get_toencode_files().await })?;

    files.par_iter().for_each(|file| {
        if handler.load(Ordering::SeqCst) {
            if let Err(error) = handle_encode(file) {
                eprintln!("{}", FileError::new(file, error));
            } else if let Err(error) = smol::block_on(async { conn.update_file(file).await }) {
                eprintln!("{}", FileError::new(file, error));
            }
        }
    });

    #[cfg(not(test))]
    {
        if handler.load(Ordering::SeqCst) {
            bar.finish_with_message("Finished reencoding");
        } else {
            bar.abandon_with_message("Reencoding aborted");
        }
    }
    Ok(())
}

pub fn clean_files(conn: &Database, handler: Arc<AtomicBool>) -> Result<()> {
    let files = smol::block_on(async { conn.init_clean_files().await })?;

    #[cfg(not(test))]
    let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?);

    files.par_iter().for_each(|file| {
        if handler.load(Ordering::SeqCst) && !file.exists() {
            if let Err(error) = smol::block_on(async { conn.remove_file(file).await }) {
                eprintln!("{}", FileError::new(file, error))
            };
            #[cfg(not(test))]
            spinner.inc(1);
        }
    });
    #[cfg(not(test))]
    spinner.finish();

    smol::block_on(async { conn.vaccum().await })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use macro_rules_attribute::apply;
    use smol_macros::{Executor, test};

    #[apply(test!)]
    async fn test_index_lots_of_files(ex: &Executor<'_>) {
        ex.spawn(async {
            let handler = Arc::new(AtomicBool::new(true));
            let conn = Database::new("temp3.db").await.unwrap();
            index_files_recursively(Path::new("./testfiles"), &conn, handler).unwrap();
            std::fs::remove_file("temp3.db").unwrap();
        })
        .await;
    }

    #[apply(test!)]
    async fn test_reencode_lots_of_files(ex: &Executor<'_>) {
        ex.spawn(async {
            let handler = Arc::new(AtomicBool::new(true));
            let conn = Database::new("temp4.db").await.unwrap();
            let temp = handler.clone();
            index_files_recursively(Path::new("./testfiles"), &conn, temp).unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
            reencode_files(&conn, handler).unwrap();
            println!("\n{}", conn.get_toencode_number().await.unwrap());
            std::fs::remove_file("temp4.db").unwrap();
        })
        .await;
    }
}
