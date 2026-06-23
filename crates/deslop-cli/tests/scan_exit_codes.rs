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
