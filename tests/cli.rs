mod common;

use common::{paths, write_jsonl};
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

fn command(root: &std::path::Path) -> Command {
    let paths = paths(root);
    let mut command = Command::new(env!("CARGO_BIN_EXE_agents-sessions"));
    command
        .env("CLAUDE_DATA_DIR", paths.claude)
        .env("CODEX_HOME", paths.codex)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("PI_CODING_AGENT_SESSION_DIR", paths.pi_sessions);
    command
}

#[test]
fn help_and_argument_errors_match_the_cli_contract() {
    let root = tempfile::tempdir().unwrap();
    for flag in ["-h", "--help"] {
        let output = command(root.path()).arg(flag).output().unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .starts_with("Usage: agents-sessions [COMMAND] [OPTIONS]")
        );
        assert!(output.stderr.is_empty());
    }
    for name in ["launch", "delete"] {
        let output = command(root.path()).arg(name).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires a session ID argument"));
    }
    let output = command(root.path()).arg("unknown").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown command 'unknown'"));
}

#[test]
fn empty_listing_uses_stderr_and_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let output = command(root.path()).arg("ls").output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "No sessions found.\n"
    );
}

#[test]
fn listing_keeps_columns_short_ids_names_and_unicode() {
    let root = tempfile::tempdir().unwrap();
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"abcdefgh-1234","project":"/p/dir","display":"héllo 🙂","timestamp":1000}),
        ],
    );
    let output = command(root.path())
        .arg("ls")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(
        lines[0].split('\t').map(str::trim_end).collect::<Vec<_>>(),
        [
            "Agent",
            "Session ID",
            "Name",
            "Project Directory",
            "Last message",
            "Created",
            "Last Modified"
        ]
    );
    assert!(lines[1].contains("abcdefgh"));
    assert!(!lines[1].contains("abcdefgh-1234"));
    assert!(lines[1].contains("héllo 🙂"));
    assert!(lines[1].contains("1970-01-01 00:00"));
}

#[test]
fn launch_preserves_argument_boundaries_and_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project's folder");
    let bin = root.path().join("bin");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("claude");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$#\" \"$@\"\nexit 7\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"session with spaces","project":project,"display":"hi","timestamp":1000}),
        ],
    );
    let output = command(root.path())
        .args(["launch", "session"])
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    let project = project.canonicalize().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [
            project.to_str().unwrap(),
            "2",
            "--resume",
            "session with spaces"
        ]
    );
}

#[test]
fn missing_session_directory_and_executable_fail_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let output = command(root.path())
        .args(["launch", "missing"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Session 'missing' not found"));
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"id","project":root.path().join("gone"),"display":"hi","timestamp":1000}),
        ],
    );
    let output = command(root.path())
        .args(["launch", "id"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("no longer exists"));
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[json!({"sessionId":"id","project":root.path(),"display":"hi","timestamp":1000})],
    );
    let output = command(root.path())
        .args(["launch", "id"])
        .env("PATH", root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude"));
}

#[test]
fn delete_confirmation_handles_yes_no_and_eof() {
    for answer in ["y\n", "Y\n", "n\n", ""] {
        let root = tempfile::tempdir().unwrap();
        let history = paths(root.path()).claude.join("history.jsonl");
        write_jsonl(
            &history,
            &[json!({"sessionId":"id","project":"/p","display":"hi","timestamp":1000})],
        );
        let mut child = command(root.path())
            .args(["delete", "id"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let yes = answer.trim().eq_ignore_ascii_case("y");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(if yes {
            "Session 'id' deleted."
        } else {
            "Aborted."
        }));
        assert_eq!(fs::read_to_string(history).unwrap().contains("id"), !yes);
    }
}

#[test]
fn prefix_lookup_keeps_the_first_match_in_activity_order() {
    let root = tempfile::tempdir().unwrap();
    let history = paths(root.path()).claude.join("history.jsonl");
    write_jsonl(
        &history,
        &[
            json!({"sessionId":"prefix","project":"/old","display":"first","timestamp":1000}),
            json!({"sessionId":"prefix-new","project":"/new","display":"second","timestamp":2000}),
        ],
    );
    let output = command(root.path())
        .args(["delete", "prefix"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("Session ID:  prefix-new"));
}

#[test]
fn listing_and_delete_confirmation_keep_names_and_latest_prompts() {
    let root = tempfile::tempdir().unwrap();
    let data = paths(root.path());
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"target","project":"/p","display":"latest prompt","timestamp":2000}),
            json!({"sessionId":"target","project":"/p","display":"older prompt","timestamp":1000}),
            json!({"sessionId":"other","project":"/elsewhere","display":"other","timestamp":3000}),
        ],
    );
    write_jsonl(
        &data.claude.join("projects/project/target.jsonl"),
        &[json!({"type":"custom-title","customTitle":"Saved session title"})],
    );
    for args in [vec!["ls"], vec!["delete", "tar"]] {
        let output = command(root.path())
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Saved session title"), "{text}");
        assert!(text.contains("latest prompt"), "{text}");
    }
}
