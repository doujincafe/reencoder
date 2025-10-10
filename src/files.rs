use crate::db;
use crate::flac::handle_encode;
use anyhow::{Result, anyhow};
#[cfg(not(test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, sleep},
    time::{Duration, UNIX_EPOCH},
};
use tokio::fs;
use turso::{
    Database,
    transaction::{Transaction, TransactionBehavior},
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

async fn handle_file<'a>(file: &Path, tx: Transaction<'a>) -> Result<()> {
    if db::check_file(&tx, file).await? {
        let modtime = fs::metadata(&file)
            .await?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let db_modtime = db::get_modtime(&tx, file).await?;
        if modtime != db_modtime {
            db::update_file(tx, file).await?;
        }
    } else {
        db::insert_file(tx, file).await?;
    }

    Ok(())
}

pub async fn index_files_recursively(
    path: &Path,
    db: &Database,
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

    let mut tasks = tokio::task::JoinSet::new();

    #[allow(unused_variables)]
    for entry in WalkDir::new(&abspath) {
        if let Err(error) = entry {
            #[cfg(not(test))]
            bar.println(format!("{}", error));
        } else {
            let path = entry.unwrap().into_path();
            if !path.is_file() {
                continue;
            }
            if path.extension().is_some_and(|x| x == "flac") {
                let mut conn = db.connect()?;

                #[cfg(not(test))]
                let newbar = bar.clone();
                tasks.spawn(async move {
                    let tx = Transaction::new(&mut conn, TransactionBehavior::Deferred)
                        .await
                        .unwrap();
                    if let Err(error) = handle_file(&path, tx).await {
                        #[cfg(not(test))]
                        newbar.println(format!("{}", FileError::new(&path, error)));
                    } else {
                        #[cfg(not(test))]
                        newbar.inc(1);
                    }
                });
                #[cfg(not(test))]
                bar.inc_length(1);
            } else {
                break;
            }
        }
    }

    while tasks.join_next().await.is_some() {
        if !handler.load(Ordering::SeqCst) {
            tasks.shutdown().await;
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

pub fn reencode_files(
    db: &Database,
    handler: Arc<AtomicBool>,
    threads: usize,
    runtime: tokio::runtime::Runtime,
) -> Result<()> {
    let conn = db.connect()?;

    let file_vec = runtime.block_on(async { db::get_toencode_files(&conn).await })?;

    #[cfg(not(test))]
    let bar = ProgressBar::with_draw_target(
        Some(file_vec.len() as u64),
        ProgressDrawTarget::stdout_with_hz(60),
    )
    .with_style(ProgressStyle::with_template(BAR_TEMPLATE)?.progress_chars("#>-"))
    .with_message("Reencoding");
    let thread_counter = Arc::new(AtomicUsize::new(0));

    let mut files = file_vec.into_iter();

    thread::scope(|s| {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();

        #[cfg(not(test))]
        let newbar = bar.clone();

        let newhandler = handler.clone();

        s.spawn(move || {
            runtime.block_on(async {
                let mut tasks = tokio::task::JoinSet::new();

                #[allow(unused_variables)]
                while let Ok(file) = rx.recv()
                    && newhandler.load(Ordering::SeqCst)
                {
                    let mut conn = db.connect().unwrap();
                    #[cfg(not(test))]
                    let newbar = newbar.clone();
                    tasks.spawn(async move {
                        let tx = Transaction::new(&mut conn, TransactionBehavior::Deferred)
                            .await
                            .unwrap();
                        if let Err(error) = db::update_file(tx, &file).await {
                            #[cfg(not(test))]
                            newbar.println(format!("{}", FileError::new(&file, error)))
                        }
                        #[cfg(not(test))]
                        newbar.inc(1)
                    });
                }

                tasks.join_all().await;
            })
        });

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

            let newhandler = handler.clone();
            let thread_counter = thread_counter.clone();
            let tx = tx.clone();
            #[cfg(not(test))]
            let bar = bar.clone();

            s.spawn(move || {
                match handle_encode(&file, newhandler) {
                    Err(error) => eprintln!("{}", FileError::new(&file, error)),
                    Ok(false) => {
                        #[allow(unused_variables)]
                        if let Err(error) = tx.send(file.clone()) {
                            #[cfg(not(test))]
                            bar.println(format!("{}", FileError::new(&file, error.into())));
                        };
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

pub async fn clean_files(db: &Database, handler: Arc<AtomicBool>) -> Result<()> {
    let mut conn = db.connect()?;
    let files = db::fetch_files(&conn).await?;

    #[cfg(not(test))]
    let spinner = ProgressBar::with_draw_target(None, ProgressDrawTarget::stdout_with_hz(60))
        .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?);
    #[cfg(not(test))]
    spinner.tick();

    let mut tasks = tokio::task::JoinSet::new();
    for file in files {
        if handler.load(Ordering::SeqCst) {
            #[cfg(not(test))]
            let spinner = spinner.clone();

            let mut conn = db.connect().unwrap();

            #[allow(unused_variables)]
            tasks.spawn(async move {
                let tx = Transaction::new(&mut conn, TransactionBehavior::Deferred)
                    .await
                    .unwrap();
                if let Err(error) = db::remove_file(tx, &file).await {
                    #[cfg(not(test))]
                    spinner.println(format!("{}", FileError::new(&file, error)))
                } else {
                    #[cfg(not(test))]
                    spinner.inc(1);
                }
            });
        }
    }

    while tasks.join_next().await.is_some() {
        if !handler.load(Ordering::SeqCst) {
            tasks.shutdown().await;
            break;
        }
    }

    #[cfg(not(test))]
    spinner.finish();

    let tx = Transaction::new(&mut conn, TransactionBehavior::Deferred).await?;

    db::vacuum(tx).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_index_lots_of_files() {
        let dbname = PathBuf::from("temp3.db");
        let handler = Arc::new(AtomicBool::new(true));
        let db = db::init_db(Some(&dbname)).await.unwrap();
        index_files_recursively(Path::new("./testfiles"), &db, handler)
            .await
            .unwrap();
        std::fs::remove_file(dbname).unwrap();
    }

    #[tokio::test]
    async fn test_clean_files() {
        let dbname = PathBuf::from("temp4.db");
        let handler = Arc::new(AtomicBool::new(true));
        let db = db::init_db(Some(&dbname)).await.unwrap();
        let mut conn = db.connect().unwrap();
        let filenames = [
            "./samples/16bit.flac",
            "./samples/24bit.flac",
            "./samples/32bit.flac",
            "./samples/nonexisting.flac",
        ];
        std::fs::copy("./samples/32bit.flac", "./samples/nonexisting.flac").unwrap();
        for file in filenames {
            let filename = PathBuf::from(file);
            let tx = Transaction::new(&mut conn, TransactionBehavior::Deferred)
                .await
                .unwrap();
            db::insert_file(tx, &filename).await.unwrap();
        }

        std::fs::remove_file("./samples/nonexisting.flac").unwrap();

        clean_files(&db, handler).await.unwrap();
        let counter = db::fetch_files(&conn).await.unwrap().len();
        std::fs::remove_file(dbname).unwrap();
        assert!(counter == 3)
    }

    #[tokio::test]
    async fn test_reencode_lots_of_files() {
        let dbname = PathBuf::from("temp5.db");
        let handler = Arc::new(AtomicBool::new(true));
        let db = db::init_db(Some(&dbname)).await.unwrap();
        let conn = db.connect().unwrap();
        let temp = handler.clone();
        index_files_recursively(Path::new("./testfiles"), &db, temp)
            .await
            .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        println!("\n{}", db::get_toencode_number(&conn).await.unwrap());
        reencode_files(&db, handler, 4, runtime).unwrap();
        println!("\n{}", db::get_toencode_number(&conn).await.unwrap());
        std::fs::remove_file(dbname).unwrap();
    }
}
