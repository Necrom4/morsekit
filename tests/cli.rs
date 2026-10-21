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

#[test]
fn invalid_zero_pitch_reports_error_and_exits_with_status_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .arg("--pitch")
        .arg("0")
        .output()
        .expect("--pitch should launch with invalid arguments");
    let stderr = String::from_utf8(output.stderr).expect("output should contain error");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("--pitch"));
    assert!(stderr.contains("error:"));
}

#[test]
fn invalid_negative_pitch_reports_error_and_exits_with_status_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .arg("--pitch=-10")
        .output()
        .expect("--pitch should launch with invalid arguments");
    let stderr = String::from_utf8(output.stderr).expect("output should contain error");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("--pitch"));
    assert!(stderr.contains("error:"));
}

#[test]
fn word_gap_threshold_smaller_than_letter_gap_threshold_reports_error_and_exits_with_status_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_morsekit"))
        .args(["--word-gap-threshold", "1", "--letter-gap-threshold", "3"])
        .output()
        .expect("morsekit should launch with invalid arguments");
    let stderr = String::from_utf8(output.stderr).expect("output should contain error");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(stderr.contains("error:"));
    assert!(stderr.contains("--word-gap-threshold"));
    assert!(stderr.contains("--letter-gap-threshold"));
}
