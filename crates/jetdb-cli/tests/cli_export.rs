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
// Basic export
// ---------------------------------------------------------------------------

#[test]
fn export_basic() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // Should have a header line
    assert!(!lines.is_empty(), "should have at least a header line");
    // Header should start with known column names
    assert!(
        lines[0].starts_with("A,B,"),
        "header should start with known column names, got: {}",
        lines[0]
    );
    // Should have data rows beyond the header
    assert!(lines.len() > 1, "should have data rows beyond header");
}

// ---------------------------------------------------------------------------
// --no-header (-H)
// ---------------------------------------------------------------------------

#[test]
fn export_no_header() {
    let path = skip_if_missing!("V2003/testV2003.mdb");

    // Get output with header
    let with_header = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    let stdout_with = String::from_utf8_lossy(&with_header.stdout);
    let lines_with: Vec<&str> = stdout_with.lines().collect();

    // Get output without header
    let without_header = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-H"])
        .output()
        .expect("failed to run jetdb");
    assert!(without_header.status.success());
    let stdout_without = String::from_utf8_lossy(&without_header.stdout);
    let lines_without: Vec<&str> = stdout_without.lines().collect();

    // Without header should have one less line
    assert_eq!(
        lines_with.len(),
        lines_without.len() + 1,
        "no-header should have one fewer line"
    );
}

// ---------------------------------------------------------------------------
// Tab delimiter (-d)
// ---------------------------------------------------------------------------

#[test]
fn export_tab_delimiter() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-d", "\t"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let header = stdout.lines().next().unwrap_or("");
    assert!(
        header.contains('\t'),
        "header should contain tabs with -d tab, got: {header}"
    );
}

// ---------------------------------------------------------------------------
// Date format (-D)
// ---------------------------------------------------------------------------

#[test]
fn export_date_format() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-D", "%d/%m/%Y"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with custom date format, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("21/09/1974"),
        "should contain date in d/m/Y format"
    );
}

// ---------------------------------------------------------------------------
// NULL string (-0)
// ---------------------------------------------------------------------------

#[test]
fn export_null_string() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-0", "(null)"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with custom null string, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Boolean words (-B)
// ---------------------------------------------------------------------------

#[test]
fn export_boolean_words() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-B"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with boolean words, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TRUE") || stdout.contains("FALSE"),
        "should contain TRUE or FALSE with -B"
    );
}

// ---------------------------------------------------------------------------
// System columns (-s)
// ---------------------------------------------------------------------------

#[test]
fn export_system_columns() {
    let path = skip_if_missing!("V2003/testV2003.mdb");

    // Without -s
    let without = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(without.status.success());
    let header_without = String::from_utf8_lossy(&without.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    // With -s
    let with = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-s"])
        .output()
        .expect("failed to run jetdb");
    assert!(with.status.success());
    let header_with = String::from_utf8_lossy(&with.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();

    // testV2003.mdb Table1 にはレプリケーションカラムがないため、
    // -s の有無で出力は同一。フラグ受け付けの正常動作を検証。
    // レプリケーションカラムの除外ロジックはユニットテスト is_replication_* で検証済み。
    assert_eq!(
        header_with, header_without,
        "no replication columns in test data, so output should be identical"
    );
}

// ---------------------------------------------------------------------------
// RC4 CryptoAPI encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn export_rc4_cryptoapi() {
    let path = skip_if_missing!("db2007-rc4cryptoapi.accdb");
    let output = jetdb_bin()
        .args([
            "--password",
            "Test123",
            "export",
            path.to_str().unwrap(),
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("foo"), "output should contain 'foo'");
}

// ---------------------------------------------------------------------------
// NonStandard AES encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn export_nonstandard_aes() {
    let path = skip_if_missing!("db-nonstandard-aes.accdb");
    let output = jetdb_bin()
        .args([
            "--password",
            "password",
            "export",
            path.to_str().unwrap(),
            "Table_One",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test"), "output should contain 'test'");
}

// ---------------------------------------------------------------------------
// Agile encrypted .accdb (db2007-enc)
// ---------------------------------------------------------------------------

#[test]
fn export_agile_db2007() {
    let path = skip_if_missing!("db2007-enc.accdb");
    let output = jetdb_bin()
        .args([
            "--password",
            "Test123",
            "export",
            path.to_str().unwrap(),
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("foo"), "output should contain 'foo'");
}

// ---------------------------------------------------------------------------
// Encrypted .accdb — no password
// ---------------------------------------------------------------------------

#[test]
fn export_encrypted_no_password() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "SomeTable"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("password-protected"),
        "stderr should mention password-protected, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn export_nonexistent_file() {
    let output = jetdb_bin()
        .args(["export", "/nonexistent/path/to/file.mdb", "Table1"])
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
// Error: nonexistent table
// ---------------------------------------------------------------------------

#[test]
fn export_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "NoSuchTable"])
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
    assert!(
        stderr.contains("table not found: NoSuchTable"),
        "stderr should contain table name, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Jet3 file
// ---------------------------------------------------------------------------

#[test]
fn export_jet3() {
    let path = skip_if_missing!("V1997/testV1997.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with Jet3 file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().count() > 1,
        "should have header + data rows for Jet3"
    );
}

// ---------------------------------------------------------------------------
// Jet4 file
// ---------------------------------------------------------------------------

#[test]
fn export_jet4() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with Jet4 file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().count() > 1,
        "should have header + data rows for Jet4"
    );
}

// ---------------------------------------------------------------------------
// ACE12 (V2007) file
// ---------------------------------------------------------------------------

#[test]
fn export_ace12() {
    let path = skip_if_missing!("V2007/testV2007.accdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with ACE12 file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().count() > 1,
        "should have header + data rows for ACE12"
    );
}

// ---------------------------------------------------------------------------
// ACE14 (V2010) file
// ---------------------------------------------------------------------------

#[test]
fn export_ace14() {
    let path = skip_if_missing!("V2010/testV2010.accdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with ACE14 file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().count() > 1,
        "should have header + data rows for ACE14"
    );
}

// ---------------------------------------------------------------------------
// DateTimeExtended (ACE17/V2019)
// ---------------------------------------------------------------------------

#[test]
fn export_datetime_extended() {
    let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
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
        lines.len() > 1,
        "should have header + data rows, got:\n{stdout}"
    );
    // Verify date-only value (row 1): "2020-06-17"
    assert!(
        stdout.contains("2020-06-17"),
        "should contain date-only value 2020-06-17, got:\n{stdout}"
    );
    // Verify full precision value: "2021-06-14 22:45:12.3456789"
    assert!(
        stdout.contains("2021-06-14 22:45:12.3456789"),
        "should contain full precision datetime, got:\n{stdout}"
    );
}
