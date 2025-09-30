use crate::db;
use crate::flac::handle_encode;
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, sleep},
    time::{Duration, UNIX_EPOCH},
};
use walkdir::WalkDir;

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
    fn new(file: &Path, error: anyhow::Error) -> Self {
        FileError {
            file: file.to_path_buf(),
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

fn handle_file(file: &Path, conn: &Connection) -> Result<()> {
    if db::check_file(conn, file)? {
        let modtime = file
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let db_modtime = db::get_modtime(conn, file)?;
        if modtime != db_modtime {
            db::update_file(conn, file)?;
        }
        return Ok(());
    }

    db::insert_file(conn, file)?;

    Ok(())
}

pub(crate) fn index_files_recursively(
    path: &Path,
    conn: &Connection,
    handler: Arc<AtomicBool>,
) -> Result<()> {
    if !path.is_dir() {
        return Err(anyhow!("Invalid root directory"));
    }
    let abspath = path.canonicalize()?;

    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
        .with_message("Indexing");

    for entry in WalkDir::new(&abspath) {
        if handler.load(Ordering::SeqCst) {
            let path = entry?.into_path();
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
                    eprintln!("{}", FileError::new(&path, error));
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

pub(crate) fn reencode_files(
    conn: Connection,
    handler: Arc<AtomicBool>,
    threads: usize,
) -> Result<()> {
    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(
        Some(db::get_toencode_number(&conn)?),
        ProgressDrawTarget::stdout_with_hz(60),
    )
    .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
    .with_message("Reencoding");

    let mut files = db::get_toencode_files(&conn)?.into_iter();

    let lock = Arc::new(Mutex::new(conn));

    let thread_counter = Arc::new(AtomicUsize::new(0));

    thread::scope(|s| {
        while handler.load(Ordering::SeqCst) {
            if thread_counter.load(Ordering::Relaxed) >= threads {
                sleep(Duration::from_millis(100));
                #[cfg(not(test))]
                bar.tick();
                continue;
            }

            let file = match files.next() {
                Some(file) => file,
                None => break,
            };

            thread_counter.fetch_add(1, Ordering::Relaxed);

            let handler = handler.clone();
            let lock = lock.clone();
            #[cfg(not(test))]
            let bar = bar.clone();
            let thread_counter = thread_counter.clone();

            s.spawn(move || {
                match handle_encode(&file, handler) {
                    Err(error) => eprintln!("{}", FileError::new(&file, error)),
                    Ok(false) => {
                        if let Err(error) = db::update_file(&lock.lock().unwrap(), &file) {
                            eprintln!("{}", FileError::new(&file, error));
                        }
                        #[cfg(not(test))]
                        bar.inc(1)
                    }
                    Ok(true) => {}
                };
                thread_counter.fetch_sub(1, Ordering::Relaxed);
            });
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

pub(crate) fn clean_files(conn: &Connection, handler: Arc<AtomicBool>) -> Result<()> {
    let files = db::init_clean_files(conn)?;

    #[cfg(not(test))]
    let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?);
    #[cfg(not(test))]
    spinner.tick();

    files.iter().for_each(|file| {
        #[allow(clippy::collapsible_if)]
        if handler.load(Ordering::SeqCst) && !file.exists() {
            if let Err(error) = db::remove_file(conn, file) {
                eprintln!("{}", FileError::new(file, error))
            };
            #[cfg(not(test))]
            spinner.inc(1);
        }
    });
    #[cfg(not(test))]
    spinner.finish();

    db::vacuum(conn)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_lots_of_files() {
        let dbname = PathBuf::from("temp3.db");
        let handler = Arc::new(AtomicBool::new(true));
        let conn = db::init_connection(Some(&dbname)).unwrap();
        index_files_recursively(Path::new("./testfiles"), &conn, handler).unwrap();
        std::fs::remove_file(dbname).unwrap();
    }

    #[test]
    fn test_clean_files() {
        let dbname = PathBuf::from("temp4.db");
        let handler = Arc::new(AtomicBool::new(true));
        let conn = db::init_connection(Some(&dbname)).unwrap();
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
            "./samples/nonexisting.flac",
        ];
        std::fs::copy("./samples/32bit.flac", "./samples/nonexisting.flac").unwrap();
        for file in filenames {
            let filename = PathBuf::from(file);
            db::insert_file(&conn, &filename).unwrap();
        }

        std::fs::remove_file("./samples/nonexisting.flac").unwrap();

        clean_files(&conn, handler).unwrap();
        let counter = db::init_clean_files(&conn).unwrap().len();
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 3)
    }

    #[test]
    fn test_reencode_lots_of_files() {
        let dbname = PathBuf::from("temp5.db");
        let handler = Arc::new(AtomicBool::new(true));
        let conn = db::init_connection(Some(&dbname)).unwrap();
        let temp = handler.clone();
        index_files_recursively(Path::new("./testfiles"), &conn, temp).unwrap();
        println!("\n{}", db::get_toencode_number(&conn).unwrap());
        reencode_files(conn, handler, 4).unwrap();
        let conn = db::init_connection(Some(&dbname)).unwrap();
        println!("\n{}", db::get_toencode_number(&conn).unwrap());
        std::fs::remove_file(dbname).unwrap();
    }
}
