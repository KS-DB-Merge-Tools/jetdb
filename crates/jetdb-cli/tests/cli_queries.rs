use std::path::PathBuf;
use std::process::Command;

/// Resolve the path to a test data file, returning `None` if missing.
fn test_data_path(relative: &str) -> Option<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir)
        .join("../../testdata")
        .join(relative);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

macro_rules! skip_if_missing {
    ($path:expr) => {
        match test_data_path($path) {
            Some(p) => p,
            None => {
                eprintln!("SKIP: test data not found: {}", $path);
                return;
            }
        }
    };
}

fn jetdb_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetdb"))
}

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
