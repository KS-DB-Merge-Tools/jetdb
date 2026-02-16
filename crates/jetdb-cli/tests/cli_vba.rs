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
// List VBA modules (default: space-separated)
// ---------------------------------------------------------------------------

#[test]
fn vba_list() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(!trimmed.is_empty(), "should have module names in output");
    assert!(
        trimmed.contains("Module1"),
        "should contain Module1, got: {trimmed}"
    );
    assert!(
        trimmed.contains("Class1"),
        "should contain Class1, got: {trimmed}"
    );
    assert!(
        trimmed.contains("Form_Form1"),
        "should contain Form_Form1, got: {trimmed}"
    );
}

// ---------------------------------------------------------------------------
// List VBA modules with newline delimiter (-1)
// ---------------------------------------------------------------------------

#[test]
fn vba_list_newline() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap(), "-1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 3,
        "should have at least 3 module names, got {}",
        lines.len()
    );
}

// ---------------------------------------------------------------------------
// List VBA modules with custom delimiter (-d)
// ---------------------------------------------------------------------------

#[test]
fn vba_list_delimiter() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap(), "-d", "|"])
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
// Show VBA module source code
// ---------------------------------------------------------------------------

#[test]
fn vba_show_source() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "show", path.to_str().unwrap(), "Module1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Function"),
        "should contain VBA keyword 'Function', got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent module
// ---------------------------------------------------------------------------

#[test]
fn vba_nonexistent_module() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "show", path.to_str().unwrap(), "NoSuch"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent module"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("VBA module not found: NoSuch"),
        "stderr should contain module name, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn vba_nonexistent_file() {
    let output = jetdb_bin()
        .args(["vba", "list", "/nonexistent/path/to/file.mdb"])
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
// No VBA in database → empty output (success)
// ---------------------------------------------------------------------------

#[test]
fn vba_no_vba() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed even with no VBA, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "should have empty output for database with no VBA, got: {stdout}"
    );
}
