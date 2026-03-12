#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// List forms
// ---------------------------------------------------------------------------

#[test]
fn form_list() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(
        trimmed.contains("Form1"),
        "should contain Form1, got: {trimmed}"
    );
}

#[test]
fn form_list_newline() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.iter().any(|l| l.trim() == "Form1"),
        "should have Form1 in output"
    );
}

// ---------------------------------------------------------------------------
// List forms - no forms in database
// ---------------------------------------------------------------------------

#[test]
fn form_list_no_forms() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim().is_empty(), "should be empty for no-forms DB");
}

// ---------------------------------------------------------------------------
// Dump form blob
// ---------------------------------------------------------------------------

#[test]
fn form_dump_blob() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "dump", path.to_str().unwrap(), "Form1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() >= 2,
        "Blob should be at least 2 bytes, got {}",
        output.stdout.len()
    );
    // Verify Blob header (common pattern)
    assert_eq!(
        output.stdout[0..2],
        [0x15, 0x00],
        "Blob should start with 0x15 0x00"
    );
}

// ---------------------------------------------------------------------------
// Dump form not found
// ---------------------------------------------------------------------------

#[test]
fn form_dump_not_found() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "dump", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report not found, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Controls not found
// ---------------------------------------------------------------------------

#[test]
fn form_controls_not_found() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "controls", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
}
