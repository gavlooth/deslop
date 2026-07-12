use std::fs;
use std::process::Command;

#[test]
fn fail_on_major_exits_nonzero_on_sloppy_and_zero_on_clean() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sloppy = temp.path().join("sloppy.rs");
    let clean = temp.path().join("clean.rs");
    fs::write(
        &sloppy,
        "fn unfinished() -> i32 {\n    todo!(\"TODO: implement\")\n}\n",
    )
    .expect("write sloppy");
    fs::write(&clean, "fn finished() -> i32 {\n    1\n}\n").expect("write clean");

    let sloppy_status = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .args(["scan", "--fail-on", "major"])
        .arg(&sloppy)
        .status()
        .expect("run sloppy scan");
    assert!(
        !sloppy_status.success(),
        "sloppy scan should fail on major finding"
    );

    let clean_status = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .args(["scan", "--fail-on", "major"])
        .arg(&clean)
        .status()
        .expect("run clean scan");
    assert!(
        clean_status.success(),
        "clean scan should not fail on major findings"
    );
}

#[test]
fn malformed_scan_returns_incomplete_exit_with_structured_output() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/typescript/malformed.ts");
    let output = Command::new(env!("CARGO_BIN_EXE_deslop"))
        .args(["scan", "--format", "json"])
        .arg(fixture)
        .output()
        .expect("run malformed scan");

    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("scan JSON");
    assert_eq!(value["schema"], "deslop.findings/2");
    assert_eq!(value["status"], "partial");
    assert!(
        value["reports"][0]["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
