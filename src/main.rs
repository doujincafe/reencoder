mod db;
mod files;
mod flac;
use anyhow::Error;
use clap::{Arg, ArgAction, command, value_parser};
use std::path::PathBuf;

fn main() -> Result<(), Error> {
    let matches = command!()
        .help_expected(true)
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .value_parser(value_parser!(PathBuf))
                .help("Path for indexing/reencoding"),
        )
        .arg(
            Arg::new("index")
                .short('i')
                .long("index")
                .action(ArgAction::SetTrue)
                .requires("path")
                .help("Only index files"),
        )
        .arg(
            Arg::new("doit")
                .long("doit")
                .action(ArgAction::SetTrue)
                .conflicts_with("index")
                .help("Actually reencode"),
        )
        .arg(
            Arg::new("clean")
                .short('c')
                .long("clean")
                .action(ArgAction::SetTrue)
                .help("Clean and dedupe database"),
        )
        .arg(
            Arg::new("threads")
                .short('t')
                .long("threads")
                .value_parser(value_parser!(usize))
                .default_value("4")
                .help("Set number of reencoding threads (default: 4)"),
        )
        .arg(
            Arg::new("db")
                .long("db")
                .value_parser(value_parser!(PathBuf))
                .help("Path to database file"),
        )
        .get_matches();

    let threads = *matches.get_one::<usize>("threads").unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(threads)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let conn = if let Some(path) = matches.get_one::<PathBuf>("db") {
            db::Database::new(path).await?
        } else {
            db::open_default_db().await?
        };
        let path = matches.get_one::<PathBuf>("path");
        if matches.get_flag("index") {
            let folderpath = path.unwrap();
            files::index_files_recursively(folderpath, &conn).await
        } else if matches.get_flag("doit") {
            files::reencode_files(&conn, path).await
        } else if matches.get_flag("clean") {
            conn.clean_files().await
        } else {
            let count = files::count_reencode_files(&conn, path).await.unwrap();
            println!("Files to reencode:\t{count}");
            Ok(())
        }
    })?;

    Ok(())
}
