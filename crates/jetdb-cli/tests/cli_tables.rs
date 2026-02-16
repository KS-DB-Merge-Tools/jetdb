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
// Default output (no flags)
// ---------------------------------------------------------------------------

#[test]
fn tables_default() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should not contain system tables
    for line in stdout.lines() {
        assert!(
            !line.contains("MSys"),
            "default output should not contain system tables, got: {line}"
        );
    }
    // Should have at least one table
    assert!(
        !stdout.trim().is_empty(),
        "should list at least one user table"
    );
}

// ---------------------------------------------------------------------------
// --system / -s
// ---------------------------------------------------------------------------

#[test]
fn tables_system() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "-s", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|l| l == "MSysObjects"),
        "should contain MSysObjects with -s flag, got:\n{stdout}"
    );
}

#[test]
fn tables_system_long_flag() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "--system", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|l| l == "MSysObjects"),
        "should contain MSysObjects with --system flag, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --show-type / -t
// ---------------------------------------------------------------------------

#[test]
fn tables_show_type() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "-t", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        assert_eq!(
            parts.len(),
            2,
            "each line should be 'number\\tname', got: {line}"
        );
        assert!(
            parts[0].parse::<i32>().is_ok(),
            "first field should be a number, got: {}",
            parts[0]
        );
    }
}

// ---------------------------------------------------------------------------
// --show-type-name / -T
// ---------------------------------------------------------------------------

#[test]
fn tables_show_type_name() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "-T", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        assert_eq!(
            parts.len(),
            2,
            "each line should be 'typename\\tname', got: {line}"
        );
        assert_eq!(
            parts[0], "table",
            "user tables should have type name 'table', got: {}",
            parts[0]
        );
    }
}

// ---------------------------------------------------------------------------
// -s -T combined
// ---------------------------------------------------------------------------

#[test]
fn tables_system_show_type_name() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "-s", "-T", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let msys_line = stdout
        .lines()
        .find(|l| l.contains("MSysObjects"))
        .expect("should contain MSysObjects");
    assert!(
        msys_line.starts_with("systable\t"),
        "MSysObjects should be prefixed with 'systable\\t', got: {msys_line}"
    );
}

// ---------------------------------------------------------------------------
// Conflict: -t and -T together
// ---------------------------------------------------------------------------

#[test]
fn tables_conflict_t_and_big_t() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["tables", "-t", "-T", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "-t and -T should conflict and cause an error"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn tables_nonexistent_file() {
    let output = jetdb_bin()
        .args(["tables", "/nonexistent/path/to/file.mdb"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}
