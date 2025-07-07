use anyhow::{Result, anyhow};
#[cfg(not(test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use rusqlite::Connection;
use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
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
struct FileError {
    file: PathBuf,
    error: anyhow::Error,
}

impl FileError {
    fn new(file: impl AsRef<Path>, error: anyhow::Error) -> Self {
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

fn handle_file(file: impl AsRef<Path>, conn: &Connection) -> Result<()> {
    if conn.check_file(&file)? {
        let modtime = file
            .as_ref()
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let db_modtime = conn.get_modtime(&file)?;
        if modtime != db_modtime {
            if let Err(error) = conn.update_file(&file) {
                return Err(FileError::new(&file, error).into());
            };
        }
        return Ok(());
    }

    if let Err(error) = conn.insert_file(&file) {
        return Err(FileError::new(file, error).into());
    }

    Ok(())
}

pub fn index_files_recursively(
    path: impl AsRef<Path>,
    conn: &Connection,
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

    for entry in WalkDir::new(&abspath) {
        if handler.load(Ordering::SeqCst) {
            let path = entry.unwrap().into_path();
            if !path.is_file() {
                continue;
            }
            if path.extension().is_some_and(|x| x == "flac") {
                #[cfg(not(test))]
                bar.inc_length(1);
            }
        } else {
            break;
        }
    }

    for entry in WalkDir::new(abspath) {
        if handler.load(Ordering::SeqCst) {
            let path = entry.unwrap().into_path();
            if !path.is_file() {
                continue;
            }
            if path.extension().is_some_and(|x| x == "flac") {
                if let Err(error) = handle_file(&path, conn) {
                    eprintln!("{}", FileError::new(path, error));
                } else {
                    #[cfg(not(test))]
                    bar.inc(1);
                }
            }
        } else {
            break;
        }
    }

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

pub fn reencode_files(conn: Connection, handler: Arc<AtomicBool>, threads: usize) -> Result<()> {
    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(
        Some(conn.get_toencode_number()?),
        ProgressDrawTarget::stdout_with_hz(60),
    )
    .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
    .with_message("Reencoding");

    let files = conn.get_toencode_files()?;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?;

    let lock = Arc::new(Mutex::new(conn));

    pool.scope(|scope| {
        for file in files {
            if handler.load(Ordering::SeqCst) {
                scope.spawn(|_| {
                    let newconn = if let Ok(conn) = lock.lock() {
                        conn
                    } else {
                        eprintln!("Error setting up lock on file:\t{}", file.to_string_lossy());
                        return;
                    };
                    if !file.exists() {
                        let _ = newconn.remove_file(&file);
                        #[cfg(not(test))]
                        bar.dec_length(1);
                        return;
                    }

                    if let Err(error) = handle_encode(&file) {
                        eprintln!("{}", FileError::new(&file, error));
                    } else {
                        if let Err(error) = newconn.update_file(&file) {
                            eprintln!("{}", FileError::new(file, error));
                        }
                        #[cfg(not(test))]
                        bar.inc(1)
                    }
                });
            } else {
                break;
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

pub fn clean_files(conn: &Connection, handler: Arc<AtomicBool>) -> Result<()> {
    let files = conn.init_clean_files()?;

    #[cfg(not(test))]
    let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?);

    files.iter().for_each(|file| {
        if handler.load(Ordering::SeqCst) && !file.exists() {
            if let Err(error) = conn.remove_file(file) {
                eprintln!("{}", FileError::new(file, error))
            };
            #[cfg(not(test))]
            spinner.inc(1);
        }
    });
    #[cfg(not(test))]
    spinner.finish();

    conn.vacuum()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_lots_of_files() {
        let dbname = "temp3.db";
        let handler = Arc::new(AtomicBool::new(true));
        let conn = Connection::new(Some(&dbname)).unwrap();
        index_files_recursively(Path::new("./testfiles"), &conn, handler).unwrap();
        std::fs::remove_file(dbname).unwrap();
    }

    #[test]
    fn test_clean_files() {
        let dbname = "temp4.db";
        let handler = Arc::new(AtomicBool::new(true));
        let conn = Connection::new(Some(&dbname)).unwrap();
        let filenames = ["16bit.flac", "24bit.flac", "32bit.flac", "nonexisting.flac"];
        std::fs::copy("32bit.flac", "nonexisting.flac").unwrap();
        for file in filenames {
            conn.insert_file(&file.to_string()).unwrap();
        }

        std::fs::remove_file("nonexisting.flac").unwrap();

        clean_files(&conn, handler).unwrap();
        let counter = conn.init_clean_files().unwrap().len();
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 3)
    }

    #[test]
    fn test_reencode_lots_of_files() {
        let dbname = "temp5.db";
        let handler = Arc::new(AtomicBool::new(true));
        let conn = Connection::new(Some(&dbname)).unwrap();
        let temp = handler.clone();
        index_files_recursively(Path::new("./testfiles"), &conn, temp).unwrap();
        println!("\n{}", conn.get_toencode_number().unwrap());
        reencode_files(conn, handler, 2).unwrap();
        let conn = Connection::new(Some(&dbname)).unwrap();
        println!("\n{}", conn.get_toencode_number().unwrap());
        std::fs::remove_file(dbname).unwrap();
    }
}
