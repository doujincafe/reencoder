mod db;
mod files;
mod flac;
use anyhow::Result;
use clap::{Arg, ArgAction, Command, ValueHint, command, value_parser};
use clap_complete::{Generator, Shell, generate};
use std::path::PathBuf;

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
                .conflicts_with("index")
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
    let args = build_cli().get_matches();

    if let Some(generator) = args.get_one::<Shell>("shell").copied() {
        let mut cmd = build_cli();
        eprintln!("Generating completion file for {generator}...");
        print_completions(generator, &mut cmd);
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(*args.get_one::<usize>("threads").unwrap())
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let conn = if let Some(path) = args.get_one::<PathBuf>("db") {
            db::Database::new(path).await?
        } else {
            db::open_default_db().await?
        };
        let path = args.get_one::<PathBuf>("path");

        if path.is_none() && !args.get_flag("clean") && !args.get_flag("doit") {
            let count = conn.get_toencode_number().await?;
            println!("Files to reencode:\t{count}");
        } else if let Some(realpath) = path {
            if !args.get_flag("doit") {
                files::index_files_recursively(realpath, &conn).await?;
            }
        }

        if args.get_flag("clean") {
            conn.clean_files().await?;
        }

        if args.get_flag("doit") {
            files::reencode_files(&conn).await?;
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
