use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Keep,
    Drop,
}

pub struct Rule {
    pub verdict: Verdict,
    pub pattern: String,
}

pub struct Rules {
    pub list: Vec<Rule>,
    set: GlobSet,
}

impl Rules {
    pub fn parse(text: &str, home: &Path) -> Result<Rules> {
        let mut list = Vec::new();
        let mut set = GlobSetBuilder::new();
        for (number, line) in text
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.trim()))
        {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (verdict, pattern) = match line.split_at(1) {
                ("+", pattern) => (Verdict::Keep, pattern.trim()),
                ("-", pattern) => (Verdict::Drop, pattern.trim()),
                _ => bail!("line {number}: rule must start with + (keep) or - (drop): {line}"),
            };
            let expanded = match pattern.strip_prefix("~/") {
                Some(rest) => home.join(rest).to_string_lossy().into_owned(),
                None => pattern.to_owned(),
            };
            set.add(
                GlobBuilder::new(&expanded)
                    .literal_separator(true)
                    .build()
                    .with_context(|| format!("line {number}: invalid pattern {pattern}"))?,
            );
            list.push(Rule {
                verdict,
                pattern: pattern.to_owned(),
            });
        }
        Ok(Rules {
            list,
            set: set.build()?,
        })
    }

    fn last_match(&self, path: &Path) -> Option<usize> {
        self.set.matches(path).into_iter().max()
    }
}

pub struct Classifier<'rules> {
    rules: &'rules Rules,
    directories: HashMap<PathBuf, Option<usize>>,
}

impl<'rules> Classifier<'rules> {
    pub fn new(rules: &'rules Rules) -> Classifier<'rules> {
        Classifier {
            rules,
            directories: HashMap::new(),
        }
    }

    pub fn classify(&mut self, path: &Path) -> Option<usize> {
        self.rules
            .last_match(path)
            .or_else(|| path.parent().and_then(|parent| self.directory(parent)))
    }

    fn directory(&mut self, directory: &Path) -> Option<usize> {
        if let Some(decision) = self.directories.get(directory) {
            return *decision;
        }
        let decision = self
            .rules
            .last_match(directory)
            .or_else(|| directory.parent().and_then(|parent| self.directory(parent)));
        self.directories.insert(directory.to_path_buf(), decision);
        decision
    }
}
