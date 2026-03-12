#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// List queries (default: space-separated)
// ---------------------------------------------------------------------------

#[test]
fn queries_list() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(!trimmed.is_empty(), "should have query names in output");
    // Should contain known query names
    assert!(
        trimmed.contains("SelectQuery"),
        "should contain SelectQuery, got: {trimmed}"
    );
    assert!(
        trimmed.contains("UnionQuery"),
        "should contain UnionQuery, got: {trimmed}"
    );
}

// ---------------------------------------------------------------------------
// List queries with newline delimiter (-1)
// ---------------------------------------------------------------------------

#[test]
fn queries_list_newline() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", path.to_str().unwrap(), "-1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // queryTestV2003.mdb has 9 queries
    assert!(
        lines.len() >= 9,
        "should have at least 9 query names, got {}",
        lines.len()
    );
}

// ---------------------------------------------------------------------------
// List queries with custom delimiter (-d)
// ---------------------------------------------------------------------------

#[test]
fn queries_list_delimiter() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", path.to_str().unwrap(), "-d", "|"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('|'),
        "should contain | delimiter, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Show SQL for a specific query
// ---------------------------------------------------------------------------

#[test]
fn queries_show_sql() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "show", path.to_str().unwrap(), "DeleteQuery"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DELETE"),
        "should contain DELETE keyword, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent query
// ---------------------------------------------------------------------------

#[test]
fn queries_nonexistent_query() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "show", path.to_str().unwrap(), "NoSuchQuery"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent query"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
    assert!(
        stderr.contains("query not found: NoSuchQuery"),
        "stderr should contain query name, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn queries_nonexistent_file() {
    let output = jetdb_bin()
        .args(["queries", "list", "/nonexistent/path/to/file.mdb"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// No queries in database → empty output
// ---------------------------------------------------------------------------

#[test]
fn queries_no_queries() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed even with no queries, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "should have empty output for database with no queries, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: -1 and -d are mutually exclusive
// ---------------------------------------------------------------------------

#[test]
fn queries_newline_delimiter_conflict() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", path.to_str().unwrap(), "-1", "-d", "|"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail when -1 and -d are both specified"
    );
}

// ---------------------------------------------------------------------------
// Japanese query name
// ---------------------------------------------------------------------------

#[test]
fn queries_japanese_name() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["queries", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|l| l == "jp_クエリ_02"),
        "should contain Japanese query name jp_クエリ_02, got:\n{stdout}"
    );
}

#[test]
fn queries_show_japanese() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["queries", "show", path.to_str().unwrap(), "jp_クエリ_02"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("jp_テーブル2"),
        "SQL should reference Japanese table name, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Jet3 (V1997): list queries
// ---------------------------------------------------------------------------

#[test]
fn queries_list_jet3() {
    let path = skip_if_missing!("V1997/queryTestV1997.mdb");
    let output = jetdb_bin()
        .args(["queries", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 9,
        "queryTestV1997.mdb should have at least 9 queries, got {}",
        lines.len()
    );
    assert!(
        stdout.contains("SelectQuery"),
        "should contain SelectQuery, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE14 (V2010): list queries
// ---------------------------------------------------------------------------

#[test]
fn queries_list_v2010() {
    let path = skip_if_missing!("V2010/queryTestV2010.accdb");
    let output = jetdb_bin()
        .args(["queries", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 9,
        "queryTestV2010.accdb should have at least 9 queries, got {}",
        lines.len()
    );
    assert!(
        stdout.contains("SelectQuery"),
        "should contain SelectQuery, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE format (V2007): list queries
// ---------------------------------------------------------------------------

#[test]
fn queries_list_v2007() {
    let path = skip_if_missing!("V2007/queryTestV2007.accdb");
    let output = jetdb_bin()
        .args(["queries", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 9,
        "queryTestV2007.accdb should have at least 9 queries, got {}",
        lines.len()
    );
    assert!(
        stdout.contains("SelectQuery"),
        "should contain SelectQuery, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE format (V2007): show query SQL
// ---------------------------------------------------------------------------

#[test]
fn queries_show_sql_v2007() {
    let path = skip_if_missing!("V2007/queryTestV2007.accdb");
    let output = jetdb_bin()
        .args(["queries", "show", path.to_str().unwrap(), "DeleteQuery"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DELETE"),
        "should contain DELETE keyword, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// formPropTest.accdb: queries list should return results (regression test)
// ---------------------------------------------------------------------------

#[test]
fn queries_list_form_prop_test() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["queries", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // 3 queries in catalog, but ~sq_fF_Table1 is an embedded query (filtered)
    assert_eq!(
        lines.len(),
        2,
        "formPropTest.accdb should have 2 user queries, got: {stdout}"
    );
}
