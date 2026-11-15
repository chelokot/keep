use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::Result;
use ignore::{WalkBuilder, WalkState};

use crate::index::{Entry, Index, Inode};

enum Found {
    File(Entry),
    Unreadable(String),
}

pub fn scan(host: String, roots: Vec<PathBuf>) -> Result<Index> {
    let mut builder = WalkBuilder::new(&roots[0]);
    for root in &roots[1..] {
        builder.add(root);
    }
    builder
        .standard_filters(false)
        .follow_links(false)
        .same_file_system(true);
    let (sender, receiver) = mpsc::channel();
    builder.build_parallel().run(|| {
        let sender = sender.clone();
        Box::new(move |result| {
            let found = match result {
                Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_dir()) => {
                    return WalkState::Continue;
                }
                Ok(entry) => match entry.metadata() {
                    Ok(metadata) => Found::File(Entry {
                        size: metadata.len(),
                        inode: Inode {
                            device: metadata.dev(),
                            number: metadata.ino(),
                        },
                        path: entry.into_path(),
                    }),
                    Err(error) => Found::Unreadable(error.to_string()),
                },
                Err(error) => Found::Unreadable(error.to_string()),
            };
            sender.send(found).expect("scan receiver outlives walkers");
            WalkState::Continue
        })
    });
    drop(sender);
    let mut entries = Vec::new();
    let mut unreadable = Vec::new();
    for found in receiver {
        match found {
            Found::File(entry) => entries.push(entry),
            Found::Unreadable(message) => unreadable.push(message),
        }
    }
    entries.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    unreadable.sort_unstable();
    Ok(Index {
        host,
        roots,
        entries,
        unreadable,
    })
}
