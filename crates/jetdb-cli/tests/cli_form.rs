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
        !output.stdout.is_empty(),
        "Blob should not be empty"
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
// Controls (TypeInfo)
// ---------------------------------------------------------------------------

#[test]
fn form_controls() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "controls", path.to_str().unwrap(), "Form1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "should have control entries");
}

#[test]
fn form_controls_not_found() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "controls", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
}

// ---------------------------------------------------------------------------
// ACE14 (formPropTest.accdb)
// ---------------------------------------------------------------------------

#[test]
fn form_list_ace14() {
    let path = skip_if_missing!("formPropTest.accdb");
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
    assert!(
        stdout.contains("F_Table1"),
        "should contain F_Table1, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Integration tests with formPropTest.accdb
// ---------------------------------------------------------------------------

#[test]
fn form_list_form_prop_test() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert_eq!(trimmed, "F_Buttons F_Table0 F_Table1 jp_フォーム_2");
}

#[test]
fn form_list_newline_form_prop_test() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, ["F_Buttons", "F_Table0", "F_Table1", "jp_フォーム_2"]);
}

#[test]
fn form_props_record_source() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RecordSource"), "should contain RecordSource");
    assert!(stdout.contains("SELECT * FROM Table1;"), "should contain the SQL");
    assert!(stdout.contains("Filter"), "should contain Filter");
    assert!(stdout.contains("[ID] > 0"), "should contain the filter expression");
}

#[test]
fn form_props_control_source() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ControlSource"), "should contain ControlSource");
    assert!(stdout.contains("=[Price]*[Qty]"), "should contain calculated expression");
}

#[test]
fn form_props_japanese_form() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "jp_フォーム_2"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("jp_フォーム_2"), "should contain form name");
    assert!(stdout.contains("jp_クエリ_02"), "should contain RecordSource value");
    assert!(stdout.contains("商品名"), "should contain Japanese control name");
}

#[test]
fn form_props_empty_form() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Table0"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("F_Table0"));
    assert!(!stdout.contains("RecordSource"), "empty form should not have RecordSource");
    assert!(!stdout.contains("Filter"), "empty form should not have Filter");
}

#[test]
fn form_props_events() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Buttons"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events = [
        "OnClick", "OnGotFocus", "OnLostFocus", "OnDblClick",
        "OnMouseDown", "OnMouseUp", "OnMouseMove",
        "OnKeyDown", "OnKeyUp", "OnKeyPress",
        "OnEnter", "OnExit",
    ];
    for event_name in &events {
        assert!(stdout.contains(event_name),
            "should contain event '{}'", event_name);
    }
    let ep_count = stdout.matches("[Event Procedure]").count();
    assert!(ep_count >= 12,
        "should have at least 12 [Event Procedure] occurrences, got {}", ep_count);
}

#[test]
fn form_props_format() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Format"), "should contain Format property");
    assert!(stdout.contains("¥#,##0;-¥#,##0"), "should contain currency format");
}
