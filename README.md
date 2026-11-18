# keep

Decide, rule by rule, which files go into backups, and see how much of every machine is still undecided.

```sh
cargo install --path .
keep scan --host laptop ~                  # index one machine into ~/.local/share/keep/laptop.zst
keep report                                # kept / dropped / undecided per machine, per rule, biggest undecided directories
keep report --depth 7 --min-size 1G        # look deeper
keep files ~/.local/share/keep/laptop.zst > kept   # NUL-separated list of kept files
restic backup --files-from-raw kept
```

Rules live in `~/.config/keep/rules`, one per line:

```
+ ~/Pictures/Screenshots
- **/node_modules
```

`+` keeps, `-` drops, `#` starts a comment. A rule on a directory covers everything below it. When several rules apply to a file, the one matching the path closest to the file wins; among rules matching the same path, the later one wins. `*` stays inside one path segment, `**` spans segments, `~/` is the home directory.

Hardlinked files are counted once, as restic stores them.
