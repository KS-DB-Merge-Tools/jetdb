#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Short output (default)
// ---------------------------------------------------------------------------

#[test]
fn ver_jet3_short() {
    let path = skip_if_missing!("V1997/testV1997.mdb");
    let output = jetdb_bin()
        .args(["ver", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "JET3\n");
}

#[test]
fn ver_jet4_short() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["ver", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "JET4\n");
}

#[test]
fn ver_ace12_short() {
    let path = skip_if_missing!("V2007/testV2007.accdb");
    let output = jetdb_bin()
        .args(["ver", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ACE12\n");
}

#[test]
fn ver_ace14_short() {
    let path = skip_if_missing!("V2010/testV2010.accdb");
    let output = jetdb_bin()
        .args(["ver", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ACE14\n");
}

// ---------------------------------------------------------------------------
// Long output (--long / -l)
// ---------------------------------------------------------------------------

#[test]
fn ver_jet4_long() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["ver", "--long", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Jet4 (Access 2000/2003)\n"
    );
}

#[test]
fn ver_jet4_long_short_flag() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["ver", "-l", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Jet4 (Access 2000/2003)\n"
    );
}

// ---------------------------------------------------------------------------
// Encrypted .accdb
// ---------------------------------------------------------------------------

#[test]
fn ver_encrypted_accdb() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["--password", "1234567890", "ver", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ACE14");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn ver_nonexistent_file() {
    let output = jetdb_bin()
        .args(["ver", "/nonexistent/path/to/file.mdb"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}
