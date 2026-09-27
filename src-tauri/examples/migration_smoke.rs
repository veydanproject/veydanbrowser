// Temporary smoke check for legacy_migration helpers. Not part of the repo.
use std::path::Path;
use veydan_lib::legacy_migration::{compare_trees, copy_tree, integrity_check, walk};

fn main() {
    let root = std::env::temp_dir().join(format!("vmig-{}", std::process::id()));
    let old = root.join("old");
    let backup = root.join("backup");
    let new = root.join("new");
    std::fs::create_dir_all(old.join("profiles/p1")).unwrap();
    std::fs::create_dir_all(old.join("notes/documents")).unwrap();
    std::fs::write(old.join("install.id"), "abc").unwrap();
    std::fs::write(old.join("profiles/p1/user.js"), vec![7u8; 300_000]).unwrap();
    std::fs::write(old.join("notes/documents/a.md"), "# hi").unwrap();
    {
        let conn = rusqlite::Connection::open(old.join("profiles.db")).unwrap();
        conn.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);").unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(old.join("nowhere"), old.join("dangling")).unwrap();

    let tree = walk(&old).unwrap();
    assert_eq!(tree.len(), 4, "dangling symlink must be skipped");
    let total: u64 = tree.values().sum();

    let mut ticks = Vec::new();
    copy_tree(&old, &backup, &tree, &mut |p, d| ticks.push((p, d))).unwrap();
    assert_eq!(ticks.last().map(|t| t.1), Some(total));
    assert!(ticks.len() > 1, "progress must tick more than once");
    compare_trees(&old, &backup).unwrap();

    std::fs::rename(&old, &new).unwrap();
    assert!(!old.exists());
    compare_trees(&backup, &new).unwrap();
    integrity_check(&new.join("profiles.db")).unwrap();

    // Tamper: size mismatch and missing file must be reported.
    std::fs::write(new.join("install.id"), "abcd").unwrap();
    let err = compare_trees(&backup, &new).unwrap_err();
    assert!(err.contains("size differs"), "{err}");
    std::fs::remove_file(new.join("install.id")).unwrap();
    let err = compare_trees(&backup, &new).unwrap_err();
    assert!(err.contains("file count differs"), "{err}");

    // Corrupt db must fail integrity check.
    std::fs::write(new.join("profiles.db"), b"garbage").unwrap();
    assert!(integrity_check(&new.join("profiles.db")).is_err());
    assert!(integrity_check(Path::new("/nonexistent.db")).is_err());

    std::fs::remove_dir_all(&root).unwrap();
    println!("migration smoke: ok ({} files, {} bytes, {} ticks)", tree.len(), total, ticks.len());
}
