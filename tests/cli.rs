use std::fs;
use std::path::Path;
use std::process::Command;

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn keep(home: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_keep"))
        .env("HOME", home)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn deepest_rule_decides_and_files_lists_only_kept() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = &sandbox.path().join("home");
    write(&root.join("Pictures/Screenshots/a.png"), 300);
    write(
        &root.join("Pictures/Screenshots/tool/node_modules/dep.js"),
        200,
    );
    write(&root.join("Projects/app/node_modules/dep.js"), 100);
    write(&root.join("Projects/app/main.rs"), 50);
    let rules = sandbox.path().join("backup.rules");
    fs::write(
        &rules,
        "# start small\n+ ~/Pictures/Screenshots\n- **/node_modules\n",
    )
    .unwrap();
    let index = sandbox.path().join("index.zst");
    let root_text = root.to_str().unwrap();

    keep(
        root,
        &[
            "scan",
            "--host",
            "laptop",
            "--output",
            index.to_str().unwrap(),
            root_text,
        ],
    );
    let report = keep(
        root,
        &[
            "report",
            "--rules",
            rules.to_str().unwrap(),
            "--min-size",
            "1",
            "--depth",
            "3",
            index.to_str().unwrap(),
        ],
    );
    let files = keep(
        root,
        &[
            "files",
            "--rules",
            rules.to_str().unwrap(),
            index.to_str().unwrap(),
        ],
    );

    assert!(report.contains("keep    300.0 B  46.2%"), "{report}");
    assert!(report.contains("drop    300.0 B  46.2%"), "{report}");
    assert!(report.contains("undecided     50.0 B   7.7%"), "{report}");
    assert!(
        report.contains(&format!("50.0 B  {}", root.join("Projects/app").display())),
        "{report}"
    );
    assert!(
        !report.contains(&format!("{}\n", root.join("Pictures").display())),
        "{report}"
    );
    let listed: Vec<&str> = files.split('\0').filter(|path| !path.is_empty()).collect();
    assert_eq!(
        listed,
        vec![root.join("Pictures/Screenshots/a.png").to_str().unwrap()]
    );
}

#[test]
fn rejects_rule_without_verdict() {
    let home = tempfile::tempdir().unwrap();
    let rules = home.path().join("backup.rules");
    fs::write(&rules, "~/Pictures\n").unwrap();
    let index = home.path().join("index.zst");
    keep(
        home.path(),
        &[
            "scan",
            "--host",
            "h",
            "--output",
            index.to_str().unwrap(),
            home.path().to_str().unwrap(),
        ],
    );
    let output = Command::new(env!("CARGO_BIN_EXE_keep"))
        .env("HOME", home.path())
        .args([
            "report",
            "--rules",
            rules.to_str().unwrap(),
            index.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must start with +"));
}

#[test]
fn hardlinked_bytes_count_once() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = &sandbox.path().join("home");
    write(&root.join("store/blob"), 400);
    fs::create_dir_all(root.join("project/node_modules")).unwrap();
    fs::hard_link(
        root.join("store/blob"),
        root.join("project/node_modules/blob"),
    )
    .unwrap();
    let rules = sandbox.path().join("backup.rules");
    fs::write(&rules, "- **/node_modules\n").unwrap();
    let index = sandbox.path().join("index.zst");

    keep(
        root,
        &[
            "scan",
            "--host",
            "h",
            "--output",
            index.to_str().unwrap(),
            root.to_str().unwrap(),
        ],
    );
    let report = keep(
        root,
        &[
            "report",
            "--rules",
            rules.to_str().unwrap(),
            index.to_str().unwrap(),
        ],
    );

    assert!(report.contains("total 400.0 B"), "{report}");
}
