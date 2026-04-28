use std::process::Command;

#[test]
fn generator_runs_with_tiny_targets() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_dat0-fixtures"))
        .arg("--out")
        .arg(dir.path())
        .arg("--csv-bytes")
        .arg("4096")
        .arg("--sqlite-target-bytes")
        .arg("4096")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(dir.path().join("generated.csv").exists());
    assert!(dir.path().join("generated.parquet").exists());
    assert!(dir.path().join("generated.sqlite").exists());
}
