use agents_sessions::utils::{CommandRunner, ProcessRunner};
use std::ffi::OsString;

#[test]
fn command_stdin_is_closed_and_output_is_captured() {
    let argv = [
        "/bin/sh",
        "-c",
        "if read answer; then exit 9; fi; printf 'captured output'",
    ];
    assert!(ProcessRunner.run(&argv.map(OsString::from)).is_ok());
}

#[test]
fn command_failure_uses_the_last_stderr_line() {
    let argv = [
        "/bin/sh",
        "-c",
        "printf 'first\\nlast reason\\n' >&2; printf 'ignore stdout'; exit 3",
    ];
    let error = ProcessRunner.run(&argv.map(OsString::from)).unwrap_err();
    assert!(error.ends_with(": last reason"), "{error}");
}

#[test]
fn command_failure_falls_back_to_stdout_or_exit_code() {
    for (script, expected) in [
        ("printf 'stdout reason'; exit 2", ": stdout reason"),
        ("exit 4", ": exit code 4"),
    ] {
        let error = ProcessRunner
            .run(&["/bin/sh", "-c", script].map(OsString::from))
            .unwrap_err();
        assert!(error.ends_with(expected), "{error}");
    }
}

#[test]
fn missing_and_empty_commands_have_clear_errors() {
    assert!(
        ProcessRunner
            .run(&[])
            .unwrap_err()
            .contains("Empty agent command")
    );
    let error = ProcessRunner.run(&["/does/not/exist".into()]).unwrap_err();
    assert!(error.starts_with("/does/not/exist: "));
}
