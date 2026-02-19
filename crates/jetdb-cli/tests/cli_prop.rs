#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Normal case: prop Table1
// ---------------------------------------------------------------------------

#[test]
fn prop_table1() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // ヘッダが "Object: Table1" であること (C の変更に伴う)
    assert!(
        stdout.contains("Object: Table1"),
        "should contain object header, got:\n{stdout}"
    );

    // Table Properties / Column セクションが出力されること
    assert!(
        stdout.contains("Table Properties:") || stdout.contains("Column:"),
        "should contain property sections, got:\n{stdout}"
    );

    // GUID プロパティが含まれること
    assert!(
        stdout.contains("GUID"),
        "should contain GUID property, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Nonexistent table: empty output
// ---------------------------------------------------------------------------

#[test]
fn prop_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "NoSuchTable_XYZ"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with empty output for nonexistent object"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "should produce empty output for nonexistent object, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Nonexistent file: error
// ---------------------------------------------------------------------------

#[test]
fn prop_nonexistent_file() {
    let output = jetdb_bin()
        .args(["prop", "/nonexistent/path/to/file.mdb", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail for nonexistent file");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}
