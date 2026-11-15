mod index;
mod report;
mod rules;
mod scan;

use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::index::Index;
use crate::rules::{Classifier, Rules, Verdict};

#[derive(Parser)]
#[command(about = "Decide, rule by rule, what goes into backups")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Scan {
        #[arg(long)]
        host: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(required = true)]
        roots: Vec<PathBuf>,
    },
    Report {
        #[arg(long)]
        rules: PathBuf,
        #[arg(long, default_value = "5G", value_parser = report::parse_size)]
        min_size: u64,
        #[arg(long, default_value_t = 4)]
        depth: usize,
        #[arg(required = true)]
        indexes: Vec<PathBuf>,
    },
    Files {
        #[arg(long)]
        rules: PathBuf,
        index: PathBuf,
    },
}

fn load_rules(path: &PathBuf) -> Result<Rules> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let home = std::env::home_dir().context("home directory is unknown")?;
    Rules::parse(&text, &home)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Scan {
            host,
            output,
            roots,
        } => {
            let index = scan::scan(host, roots)?;
            index.write(&output)?;
            eprintln!(
                "{} files, {} total, {} unreadable -> {}",
                index.entries.len(),
                report::human(index.entries.iter().map(|entry| entry.size).sum()),
                index.unreadable.len(),
                output.display()
            );
        }
        Command::Report {
            rules,
            min_size,
            depth,
            indexes,
        } => {
            let rules = load_rules(&rules)?;
            let indexes = indexes
                .iter()
                .map(|path| Index::read(path))
                .collect::<Result<Vec<_>>>()?;
            print!(
                "{}",
                report::render(
                    &indexes,
                    &rules,
                    &report::Options {
                        min_bytes: min_size,
                        depth
                    }
                )
            );
        }
        Command::Files { rules, index } => {
            let rules = load_rules(&rules)?;
            let index = Index::read(&index)?;
            let mut classifier = Classifier::new(&rules);
            let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
            for entry in &index.entries {
                if classifier
                    .classify(&entry.path)
                    .is_some_and(|rule| rules.list[rule].verdict == Verdict::Keep)
                {
                    stdout.write_all(entry.path.as_os_str().as_bytes())?;
                    stdout.write_all(b"\0")?;
                }
            }
            stdout.flush()?;
        }
    }
    Ok(())
}
