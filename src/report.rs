use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::index::Index;
use crate::rules::{Classifier, Rules, Verdict};

pub struct Options {
    pub min_bytes: u64,
    pub depth: usize,
}

#[derive(Default, Clone, Copy)]
struct Tally {
    bytes: u64,
    files: u64,
}

impl Tally {
    fn add(&mut self, size: u64) {
        self.bytes += size;
        self.files += 1;
    }

    fn merge(&mut self, other: Tally) {
        self.bytes += other.bytes;
        self.files += other.files;
    }
}

#[derive(Default, Clone, Copy)]
struct Coverage {
    keep: Tally,
    drop: Tally,
    undecided: Tally,
}

impl Coverage {
    fn total(&self) -> u64 {
        self.keep.bytes + self.drop.bytes + self.undecided.bytes
    }

    fn merge(&mut self, other: Coverage) {
        self.keep.merge(other.keep);
        self.drop.merge(other.drop);
        self.undecided.merge(other.undecided);
    }
}

struct HostReport<'index> {
    host: &'index str,
    roots: &'index [PathBuf],
    coverage: Coverage,
    undecided_by_directory: HashMap<&'index Path, u64>,
    unreadable: usize,
}

pub fn render(indexes: &[Index], rules: &Rules, options: &Options) -> String {
    let mut by_rule = vec![Tally::default(); rules.list.len()];
    let hosts: Vec<HostReport> = indexes
        .iter()
        .map(|index| {
            let mut classifier = Classifier::new(rules);
            let mut coverage = Coverage::default();
            let mut undecided_by_directory: HashMap<&Path, u64> = HashMap::new();
            let mut seen_inodes = HashSet::new();
            for entry in &index.entries {
                let size = if seen_inodes.insert(entry.inode) {
                    entry.size
                } else {
                    0
                };
                match classifier.classify(&entry.path) {
                    Some(rule) => {
                        by_rule[rule].add(size);
                        match rules.list[rule].verdict {
                            Verdict::Keep => coverage.keep.add(size),
                            Verdict::Drop => coverage.drop.add(size),
                        }
                    }
                    None => {
                        coverage.undecided.add(size);
                        let root = index.roots.iter().find(|root| entry.path.starts_with(root));
                        for ancestor in entry.path.ancestors().skip(1) {
                            if root.is_none_or(|root| !ancestor.starts_with(root)) {
                                break;
                            }
                            *undecided_by_directory.entry(ancestor).or_default() += size;
                        }
                    }
                }
            }
            HostReport {
                host: &index.host,
                roots: &index.roots,
                coverage,
                undecided_by_directory,
                unreadable: index.unreadable.len(),
            }
        })
        .collect();

    let mut out = String::new();
    writeln!(out, "COVERAGE").unwrap();
    let mut overall = Coverage::default();
    for host in &hosts {
        write_coverage(&mut out, host.host, host.coverage);
        overall.merge(host.coverage);
    }
    if hosts.len() > 1 {
        write_coverage(&mut out, "all", overall);
    }

    writeln!(out, "\nRULES").unwrap();
    for (rule, tally) in rules.list.iter().zip(&by_rule) {
        let sign = match rule.verdict {
            Verdict::Keep => '+',
            Verdict::Drop => '-',
        };
        writeln!(
            out,
            "  {sign} {:<48} {:>10} {:>12} files",
            rule.pattern,
            human(tally.bytes),
            tally.files
        )
        .unwrap();
    }

    writeln!(
        out,
        "\nBIGGEST UNDECIDED (>= {}, depth {})",
        human(options.min_bytes),
        options.depth
    )
    .unwrap();
    for host in &hosts {
        let mut children: HashMap<&Path, Vec<(&Path, u64)>> = HashMap::new();
        for (&directory, &bytes) in &host.undecided_by_directory {
            if let Some(parent) = directory.parent() {
                children.entry(parent).or_default().push((directory, bytes));
            }
        }
        for siblings in children.values_mut() {
            siblings.sort_unstable_by(|left, right| right.1.cmp(&left.1));
        }
        writeln!(out, "  {}", host.host).unwrap();
        for root in host.roots {
            let bytes = host
                .undecided_by_directory
                .get(root.as_path())
                .copied()
                .unwrap_or_default();
            write_tree(&mut out, &children, root, bytes, 0, options);
        }
        if host.unreadable > 0 {
            writeln!(
                out,
                "    ({} paths could not be read during scan)",
                host.unreadable
            )
            .unwrap();
        }
    }
    out
}

fn write_coverage(out: &mut String, label: &str, coverage: Coverage) {
    let total = coverage.total().max(1) as f64;
    let part = |tally: Tally| {
        format!(
            "{:>10} {:>5.1}%",
            human(tally.bytes),
            tally.bytes as f64 * 100.0 / total
        )
    };
    writeln!(
        out,
        "  {label:<24} keep {}   drop {}   undecided {}   total {}",
        part(coverage.keep),
        part(coverage.drop),
        part(coverage.undecided),
        human(coverage.total())
    )
    .unwrap();
}

fn write_tree(
    out: &mut String,
    children: &HashMap<&Path, Vec<(&Path, u64)>>,
    directory: &Path,
    bytes: u64,
    level: usize,
    options: &Options,
) {
    if bytes < options.min_bytes {
        return;
    }
    let indent = "  ".repeat(level + 2);
    writeln!(out, "{indent}{:>10}  {}", human(bytes), directory.display()).unwrap();
    if level == options.depth {
        return;
    }
    for &(child, child_bytes) in children
        .get(directory)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        write_tree(out, children, child, child_bytes, level + 1, options);
    }
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn parse_size(text: &str) -> Result<u64, String> {
    let (number, multiplier) = match text.trim().char_indices().last() {
        Some((position, 'K' | 'k')) => (&text[..position], 1u64 << 10),
        Some((position, 'M' | 'm')) => (&text[..position], 1 << 20),
        Some((position, 'G' | 'g')) => (&text[..position], 1 << 30),
        Some((position, 'T' | 't')) => (&text[..position], 1 << 40),
        _ => (text, 1),
    };
    number
        .trim()
        .parse::<f64>()
        .map(|value| (value * multiplier as f64) as u64)
        .map_err(|error| format!("invalid size {text}: {error}"))
}
