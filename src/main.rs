mod index;
mod report;
mod rules;
mod scan;

use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

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
    #[command(about = "Index every file under the roots of one machine")]
    Scan {
        #[arg(long, help = "Machine name shown in reports")]
        host: String,
        #[arg(long, help = "Index file [default: ~/.local/share/keep/<HOST>.zst]")]
        output: Option<PathBuf>,
        #[arg(required = true, help = "Directories to index")]
        roots: Vec<PathBuf>,
    },
    #[command(
        about = "Show how much is kept, dropped and undecided, and the biggest undecided directories"
    )]
    Report {
        #[arg(long, help = "Rules file [default: ~/.config/keep/rules]")]
        rules: Option<PathBuf>,
        #[arg(long, default_value = "5G", value_parser = report::parse_size, help = "Hide undecided directories smaller than this")]
        min_size: u64,
        #[arg(long, default_value_t = 4, help = "How deep the undecided tree goes")]
        depth: usize,
        #[arg(help = "Index files [default: all in ~/.local/share/keep]")]
        indexes: Vec<PathBuf>,
    },
    #[command(about = "Print kept files of one index, NUL-separated, for restic --files-from-raw")]
    Files {
        #[arg(long, help = "Rules file [default: ~/.config/keep/rules]")]
        rules: Option<PathBuf>,
        index: PathBuf,
    },
}

fn load_rules(path: Option<PathBuf>, home: &Path) -> Result<Rules> {
    let path = path.unwrap_or_else(|| home.join(".config/keep/rules"));
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Rules::parse(&text, home)
}

fn main() -> Result<()> {
    let home = std::env::home_dir().context("home directory is unknown")?;
    let index_directory = home.join(".local/share/keep");
    match Cli::parse().command {
        Command::Scan {
            host,
            output,
            roots,
        } => {
            let output = match output {
                Some(output) => output,
                None => {
                    std::fs::create_dir_all(&index_directory)
                        .with_context(|| format!("creating {}", index_directory.display()))?;
                    index_directory.join(format!("{host}.zst"))
                }
            };
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
            let rules = load_rules(rules, &home)?;
            let indexes = if indexes.is_empty() {
                let mut found = std::fs::read_dir(&index_directory)
                    .with_context(|| format!("reading {}", index_directory.display()))?
                    .map(|entry| Ok(entry?.path()))
                    .collect::<Result<Vec<_>>>()?;
                found.retain(|path| path.extension().is_some_and(|extension| extension == "zst"));
                found.sort();
                found
            } else {
                indexes
            };
            let indexes = std::thread::scope(|scope| {
                let reads: Vec<_> = indexes
                    .iter()
                    .map(|path| scope.spawn(|| Index::read(path)))
                    .collect();
                reads
                    .into_iter()
                    .map(|read| read.join().expect("index read panicked"))
                    .collect::<Result<Vec<_>>>()
            })?;
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
            let rules = load_rules(rules, &home)?;
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
