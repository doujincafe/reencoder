mod db;
mod files;
mod flac;
use anyhow::Result;
use clap::{Arg, ArgAction, Command, ValueHint, command, value_parser};
use clap_complete::{Generator, Shell, generate};
use console::style;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn build_cli() -> Command {
    command!()
        .arg(
            Arg::new("path")
                .help("Path for indexing/reencoding")
                .action(ArgAction::Set)
                .value_hint(ValueHint::DirPath)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("doit")
                .long("doit")
                .help("Actually reencode files")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("clean")
                .short('c')
                .long("clean")
                .help("Clean and dedupe database")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("threads")
                .short('t')
                .long("threads")
                .help("Set number of reencoding threads")
                .action(ArgAction::Set)
                .value_hint(ValueHint::Other)
                .value_parser(value_parser!(usize))
                .default_value("4"),
        )
        .arg(
            Arg::new("db")
                .short('d')
                .long("db")
                .help("Path to databse file")
                .action(ArgAction::Set)
                .value_hint(ValueHint::FilePath)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("shell")
                .short('g')
                .long("generate")
                .help("Generate shell completions")
                .action(ArgAction::Set)
                .value_parser(value_parser!(Shell)),
        )
}

fn print_completions<G: Generator>(generator: G, cmd: &mut Command) {
    generate(
        generator,
        cmd,
        cmd.get_name().to_string(),
        &mut std::io::stdout(),
    );
}

fn main() -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    let args = build_cli().get_matches();

    if let Some(generator) = args.get_one::<Shell>("shell").copied() {
        let mut cmd = build_cli();
        eprintln!("Generating completion file for {generator}...");
        print_completions(generator, &mut cmd);
        return Ok(());
    }

    let conn = if let Some(path) = args.get_one::<PathBuf>("db") {
        smol::block_on(async { db::Database::new(path).await })?
    } else {
        smol::block_on(async { db::open_default_db().await })?
    };

    let path = args.get_one::<PathBuf>("path");

    if path.is_none() && !args.get_flag("clean") && !args.get_flag("doit") {
        let count = smol::block_on(async { conn.get_toencode_number().await })?;
        println!("Files to reencode:\t{}", style(count).green());
        return Ok(());
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(*args.get_one::<usize>("threads").unwrap())
        .build()
        .unwrap();

    if let Some(realpath) = path {
        let hanlder = running.clone();
        pool.install(|| files::index_files_recursively(realpath, &conn, hanlder))?;
    }

    if args.get_flag("clean") {
        let handler = running.clone();
        pool.install(|| files::clean_files(&conn, handler))?;
    }

    if args.get_flag("doit") {
        let hanlder = running.clone();
        pool.install(|| files::reencode_files(&conn, hanlder))?;
    }
    Ok::<(), anyhow::Error>(())
}
