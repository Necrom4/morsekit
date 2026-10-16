use std::process::Command;

#[test]
fn help_flag_prints_usage_and_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .arg("--help")
        .output()
        .expect("--help should run successfully");
    let stdout = String::from_utf8(output.stdout).expect("output should contain valid UTF-8");

    assert!(output.status.success());
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("--wpm"));
    assert!(output.stderr.is_empty());
}

#[test]
fn version_flag_prints_package_version_and_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .arg("--version")
        .output()
        .expect("--version should run successfully");
    let stdout = String::from_utf8(output.stdout).expect("output should contain valid UTF-8");

    assert!(output.status.success());
    assert_eq!(
        stdout.trim(),
        format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_wpm_reports_error_and_exits_with_status_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .arg("--wpm")
        .arg("0")
        .output()
        .expect("--wpm should launch with invalid arguments");
    let stderr = String::from_utf8(output.stderr).expect("output should contain error");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("--wpm"));
    assert!(stderr.contains("error:"));
}
