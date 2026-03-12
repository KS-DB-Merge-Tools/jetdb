#[macro_use]
mod common;
use common::jetdb_bin;

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
// Jet3 (V1997)
// ---------------------------------------------------------------------------

#[test]
fn tables_jet3() {
    let path = skip_if_missing!("V1997/testV1997.mdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "should list at least one user table");
    for line in stdout.lines() {
        assert!(!line.contains("MSys"), "default output should not contain system tables");
    }
}

// ---------------------------------------------------------------------------
// ACE12 (V2007) non-encrypted
// ---------------------------------------------------------------------------

#[test]
fn tables_ace12() {
    let path = skip_if_missing!("V2007/testV2007.accdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "should list at least one user table");
    for line in stdout.lines() {
        assert!(!line.contains("MSys"), "default output should not contain system tables");
    }
}

// ---------------------------------------------------------------------------
// ACE14 (V2010)
// ---------------------------------------------------------------------------

#[test]
fn tables_ace14() {
    let path = skip_if_missing!("V2010/testV2010.accdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "should list at least one user table");
}

// ---------------------------------------------------------------------------
// ACE17 (V2019)
// ---------------------------------------------------------------------------

#[test]
fn tables_ace17() {
    let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "should list at least one user table");
}

// ---------------------------------------------------------------------------
// Encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn tables_encrypted_accdb() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["--password", "1234567890", "tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
}

#[test]
fn tables_encrypted_no_password() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["tables", path.to_str().unwrap()])
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
// RC4 CryptoAPI encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn tables_rc4_cryptoapi() {
    let path = skip_if_missing!("db2007-rc4cryptoapi.accdb");
    let output = jetdb_bin()
        .args(["--password", "Test123", "tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with RC4 CryptoAPI encrypted file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "should list at least one user table"
    );
}

// ---------------------------------------------------------------------------
// NonStandard AES encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn tables_nonstandard_aes() {
    let path = skip_if_missing!("db-nonstandard-aes.accdb");
    let output = jetdb_bin()
        .args(["--password", "password", "tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with NonStandard AES encrypted file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "should list at least one user table"
    );
}

// ---------------------------------------------------------------------------
// Agile encrypted .accdb (db2007-enc)
// ---------------------------------------------------------------------------

#[test]
fn tables_agile_db2007() {
    let path = skip_if_missing!("db2007-enc.accdb");
    let output = jetdb_bin()
        .args(["--password", "Test123", "tables", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with Agile encrypted file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "should list at least one user table"
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
