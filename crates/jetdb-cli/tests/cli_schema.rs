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
// Single table: -T Table1
// ---------------------------------------------------------------------------

#[test]
fn schema_single_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Table: Table1"),
        "should contain table header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Columns:"),
        "should contain Columns section, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Single table columns: verify specific column names and types
// ---------------------------------------------------------------------------

#[test]
fn schema_single_table_columns() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify specific columns and types
    assert!(stdout.contains("A"), "should contain column A");
    assert!(stdout.contains("Text(100)"), "should contain Text(100)");
    assert!(stdout.contains("B"), "should contain column B");
    assert!(stdout.contains("Text(200)"), "should contain Text(200)");
    assert!(stdout.contains("Long"), "should contain Long type");
    assert!(stdout.contains("Timestamp"), "should contain Timestamp type");
}

// ---------------------------------------------------------------------------
// All tables (no -T)
// ---------------------------------------------------------------------------

#[test]
fn schema_all_tables() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain at least one "Table:" header
    assert!(
        stdout.contains("Table:"),
        "should contain at least one Table: header, got:\n{stdout}"
    );
    // Should not contain system tables
    assert!(
        !stdout.contains("Table: MSys"),
        "should not show system tables, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-indexes
// ---------------------------------------------------------------------------

#[test]
fn schema_no_indexes() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-indexes",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Columns:"),
        "should still show Columns section, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Indexes:"),
        "should not contain Indexes section with --no-indexes, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-relations (with a DB that actually has relationships)
// ---------------------------------------------------------------------------

#[test]
fn schema_no_relations() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-relations",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Relationships:"),
        "should not contain Relationships section with --no-relations, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-relations effective (DB with real relationships)
// ---------------------------------------------------------------------------

#[test]
fn schema_no_relations_effective() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    // First verify relationships exist without the flag
    let output_with = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output_with.status.success());
    let stdout_with = String::from_utf8_lossy(&output_with.stdout);
    assert!(
        stdout_with.contains("Relationships:"),
        "should show Relationships without --no-relations, got:\n{stdout_with}"
    );

    // Then verify they are suppressed with the flag
    let output_without = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-relations",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output_without.status.success());
    let stdout_without = String::from_utf8_lossy(&output_without.stdout);
    assert!(
        !stdout_without.contains("Relationships:"),
        "should not contain Relationships section with --no-relations, got:\n{stdout_without}"
    );
}

// ---------------------------------------------------------------------------
// Relationships section
// ---------------------------------------------------------------------------

#[test]
fn schema_with_relationships() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Relationships:"),
        "should contain Relationships section, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Table1.otherfk1 -> Table2.id"),
        "should contain Table1.otherfk1 -> Table2.id, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-indexes --no-relations (columns only)
// ---------------------------------------------------------------------------

#[test]
fn schema_no_indexes_no_relations() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-indexes",
            "--no-relations",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Columns:"),
        "should contain Columns section, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Indexes:"),
        "should not contain Indexes section, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Relationships:"),
        "should not contain Relationships section, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn schema_nonexistent_file() {
    let output = jetdb_bin()
        .args(["schema", "/nonexistent/path/to/file.mdb"])
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
// Error: nonexistent table with specific message
// ---------------------------------------------------------------------------

#[test]
fn schema_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "NoSuchTable",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent table"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}

#[test]
fn schema_nonexistent_table_msg() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "NoSuchTable",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("table not found: NoSuchTable"),
        "stderr should contain specific table name, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Jet3 file
// ---------------------------------------------------------------------------

#[test]
fn schema_jet3() {
    let path = skip_if_missing!("V1997/testV1997.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Table: Table1"),
        "should contain Table1 header for Jet3 file, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Columns:"),
        "should contain Columns section for Jet3 file, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// ACE12 (V2007) file
// ---------------------------------------------------------------------------

#[test]
fn schema_ace12() {
    let path = skip_if_missing!("V2007/testV2007.accdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Table: Table1"),
        "should contain Table1 header for ACE12 file, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Columns:"),
        "should contain Columns section for ACE12 file, got:\n{stdout}"
    );
}
