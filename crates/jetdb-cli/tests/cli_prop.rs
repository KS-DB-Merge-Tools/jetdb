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
// Japanese object and column names
// ---------------------------------------------------------------------------

#[test]
fn prop_japanese_table() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "jp_テーブル2"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Object: jp_テーブル2"),
        "should contain Japanese object name, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Column: 商品名"),
        "should contain Japanese column name 商品名, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Jet3 (V1997)
// ---------------------------------------------------------------------------

#[test]
fn prop_jet3() {
    let path = skip_if_missing!("V1997/testV1997.mdb");
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
    assert!(
        stdout.contains("Object: Table1"),
        "should contain object header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Table Properties:") || stdout.contains("Column:"),
        "should contain property sections, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE12 (V2007)
// ---------------------------------------------------------------------------

#[test]
fn prop_ace12() {
    let path = skip_if_missing!("V2007/testV2007.accdb");
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
    assert!(
        stdout.contains("Object: Table1"),
        "should contain object header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Table Properties:") || stdout.contains("Column:"),
        "should contain property sections, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE14 (formPropTest.accdb)
// ---------------------------------------------------------------------------

#[test]
fn prop_ace14() {
    let path = skip_if_missing!("formPropTest.accdb");
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
    assert!(
        stdout.contains("Object: Table1"),
        "should contain object header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Table Properties:") || stdout.contains("Column:"),
        "should contain property sections, got:\n{stdout}"
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
