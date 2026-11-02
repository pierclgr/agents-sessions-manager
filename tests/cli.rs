mod common;

use common::{paths, write_jsonl};
use rusqlite::{Connection, params};
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
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

fn listed_ids(output: &Output) -> Vec<String> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .map(|line| line.split('\t').nth(1).unwrap().trim().to_owned())
        .collect()
}

fn resume_fixture(root: &Path) -> PathBuf {
    let data = paths(root);
    let project = root.join("project's folder");
    let child = project.join("child");
    let other = root.join("other");
    let alias = root.join("alias");
    let bin = root.join("bin");
    for directory in [&child, &other, &bin, &data.codex] {
        fs::create_dir_all(directory).unwrap();
    }
    std::os::unix::fs::symlink(&project, &alias).unwrap();
    for agent in ["claude", "codex", "opencode", "pi"] {
        let executable = bin.join(agent);
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"${0##*/}\" \"$PWD\" \"$#\" \"$@\"\nexit 7\n",
        )
        .unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"claude-old","project":project,"display":"hi","timestamp":1000}),
            json!({"sessionId":"claude-new","project":alias,"display":"hi","timestamp":2000}),
            json!({"sessionId":"claude-other","project":other,"display":"hi","timestamp":9000}),
            json!({"sessionId":"claude-child","project":child,"display":"hi","timestamp":10000}),
        ],
    );
    for id in ["claude-old", "claude-new"] {
        write_jsonl(
            &data.claude.join(format!("projects/project/{id}.jsonl")),
            &[json!({"type":"custom-title","customTitle":"Fix CAFÉ bug"})],
        );
    }
    let codex = Connection::open(data.codex.join("state_1.sqlite")).unwrap();
    codex
        .execute_batch(
            "CREATE TABLE threads (id TEXT, cwd TEXT, name TEXT, first_user_message TEXT,
        created_at_ms INTEGER, updated_at_ms INTEGER, archived INTEGER, thread_source TEXT);",
        )
        .unwrap();
    for (id, directory, updated) in [
        ("codex-old", &project, 1000),
        ("codex-new", &project, 3000),
        ("codex-other", &other, 11000),
    ] {
        codex
            .execute(
                "INSERT INTO threads VALUES (?1, ?2, 'Fix CAFÉ bug', 'hi', 1000, ?3, 0, 'user')",
                params![id, directory.to_str().unwrap(), updated],
            )
            .unwrap();
    }
    fs::create_dir_all(data.opencode_db.parent().unwrap()).unwrap();
    let opencode = Connection::open(&data.opencode_db).unwrap();
    opencode
        .execute_batch(
            "CREATE TABLE session_v2 (id TEXT, directory TEXT, title TEXT, time_created INTEGER,
            time_updated INTEGER, parent_id TEXT, time_archived INTEGER);
            CREATE TABLE session_message (session_id TEXT, data TEXT, type TEXT, seq INTEGER);",
        )
        .unwrap();
    for (id, updated) in [("opencode-old", 1000), ("opencode-new", 4000)] {
        opencode
            .execute(
                "INSERT INTO session_v2 VALUES (?1, ?2, 'Fix CAFÉ bug', 1000, ?3, NULL, NULL)",
                params![id, project.to_str().unwrap(), updated],
            )
            .unwrap();
        opencode
            .execute(
                "INSERT INTO session_message VALUES (?1, '{\"text\":\"hi\"}', 'user', 1)",
                [id],
            )
            .unwrap();
    }
    for (id, timestamp) in [
        ("pi-old", "1970-01-01T00:00:01Z"),
        ("pi-new", "1970-01-01T00:00:05Z"),
    ] {
        write_jsonl(
            &data.pi_sessions.join(format!("project/1970_{id}.jsonl")),
            &[
                json!({"type":"session","id":id,"cwd":project,"timestamp":"1970-01-01T00:00:01Z"}),
                json!({"type":"session_info","name":"Fix CAFÉ bug"}),
                json!({"type":"message","timestamp":timestamp,"message":{"role":"user","content":"hi"}}),
            ],
        );
    }
    project
}

fn assert_resumed(output: &Output, root: &Path, agent: &str, id: &str) {
    assert_eq!(
        output.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project = root.join("project's folder").canonicalize().unwrap();
    let (option, target) = match agent {
        "claude" => ("--resume", id.to_owned()),
        "codex" => ("resume", id.to_owned()),
        "opencode" => ("--session", id.to_owned()),
        "pi" => (
            "--session",
            paths(root)
                .pi_sessions
                .join(format!("project/1970_{id}.jsonl"))
                .display()
                .to_string(),
        ),
        _ => panic!("Unexpected agent: {agent}"),
    };
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [agent, project.to_str().unwrap(), "2", option, &target]
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn resume_latest_uses_cwd_activity_order_and_optional_agent_filter() {
    let root = tempfile::tempdir().unwrap();
    let project = resume_fixture(root.path());
    for cwd in [&project, &root.path().join("alias")] {
        let output = command(root.path())
            .args(["resume", "latest"])
            .current_dir(cwd)
            .env("PATH", root.path().join("bin"))
            .output()
            .unwrap();
        assert_resumed(&output, root.path(), "pi", "pi-new");
        for agent in ["claude", "codex", "opencode", "pi"] {
            let agent_option = format!("--agent={agent}");
            for args in [
                vec!["resume", "latest", "--agent", agent],
                vec!["resume", "latest", "-a", agent],
                vec!["resume", "--agent", agent, "latest"],
                vec!["resume", "latest", &agent_option],
            ] {
                let output = command(root.path())
                    .args(args)
                    .current_dir(cwd)
                    .env("PATH", root.path().join("bin"))
                    .output()
                    .unwrap();
                assert_resumed(&output, root.path(), agent, &format!("{agent}-new"));
            }
        }
    }
}

#[test]
fn resume_names_are_exact_case_insensitive_and_filtered_by_agent() {
    let root = tempfile::tempdir().unwrap();
    resume_fixture(root.path());
    for agent in ["claude", "codex", "opencode", "pi"] {
        for args in [
            vec!["resume", "fIX café BUG", "--agent", agent],
            vec!["resume", "-a", agent, "Fix CAFÉ bug"],
        ] {
            let output = command(root.path())
                .args(args)
                .current_dir(root.path())
                .env("PATH", root.path().join("bin"))
                .output()
                .unwrap();
            if agent == "codex" {
                assert_eq!(output.status.code(), Some(7));
                let text = String::from_utf8_lossy(&output.stdout);
                assert!(text.starts_with("codex\n"));
                assert!(
                    text.contains(
                        root.path()
                            .join("other")
                            .canonicalize()
                            .unwrap()
                            .to_str()
                            .unwrap()
                    )
                );
                assert!(text.ends_with("resume\ncodex-other\n"));
            } else {
                assert_resumed(&output, root.path(), agent, &format!("{agent}-new"));
            }
        }
    }
    let output = command(root.path())
        .args(["resume", "Fix CAFÉ bug"])
        .env("PATH", root.path().join("bin"))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).ends_with("resume\ncodex-other\n"));
    assert_eq!(output.status.code(), Some(7));
    for target in ["Fix CAFÉ", "missing"] {
        let output = command(root.path())
            .args(["resume", target])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("Session '{target}' not found"))
        );
    }
}

#[test]
fn resume_ids_take_priority_over_names_and_respect_agent_filters() {
    let root = tempfile::tempdir().unwrap();
    resume_fixture(root.path());
    write_jsonl(
        &paths(root.path())
            .claude
            .join("projects/project/claude-new.jsonl"),
        &[json!({"type":"custom-title","customTitle":"pi-new"})],
    );
    for args in [
        vec!["resume", "pi-new"],
        vec!["resume", "pi", "--agent", "pi"],
        vec!["resume", "pi", "-a", "pi"],
    ] {
        let output = command(root.path())
            .args(args)
            .env("PATH", root.path().join("bin"))
            .output()
            .unwrap();
        assert_resumed(&output, root.path(), "pi", "pi-new");
    }
    let output = command(root.path())
        .args(["resume", "pi-new", "--agent", "claude"])
        .env("PATH", root.path().join("bin"))
        .output()
        .unwrap();
    assert_resumed(&output, root.path(), "claude", "claude-new");
    let output = command(root.path())
        .args(["resume", "pi-new", "--agent", "codex"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Session 'pi-new' not found"));
    assert!(error.contains("codex"));
}

#[test]
fn resume_latest_reports_no_matches_without_resuming_another_directory_or_agent() {
    let root = tempfile::tempdir().unwrap();
    let project = resume_fixture(root.path());
    fs::remove_dir_all(paths(root.path()).pi_sessions).unwrap();
    for args in [
        vec!["resume", "latest"],
        vec!["resume", "latest", "-a", "pi"],
    ] {
        let output = command(root.path())
            .args(&args)
            .current_dir(if args.len() == 2 {
                root.path()
            } else {
                &project
            })
            .env("PATH", root.path().join("bin"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("No sessions found"));
        assert!(error.contains("current directory"));
        if args.len() > 2 {
            assert!(error.contains("pi"));
        }
    }
}

#[test]
fn resume_agent_filter_loads_only_the_selected_agents_data() {
    let root = tempfile::tempdir().unwrap();
    let project = resume_fixture(root.path());
    let history = paths(root.path()).claude.join("history.jsonl");
    fs::remove_file(&history).unwrap();
    // a directory cannot be read as JSONL, even when running tests as root
    fs::create_dir(history).unwrap();
    for (target, expected_id) in [
        ("codex-new", "codex-new"),
        ("Fix CAFÉ bug", "codex-other"),
        ("latest", "codex-new"),
    ] {
        let output = command(root.path())
            .args(["resume", target, "--agent", "codex"])
            .current_dir(&project)
            .env("PATH", root.path().join("bin"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).ends_with(&format!("resume\n{expected_id}\n"))
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn resume_supports_option_like_names_after_the_option_separator() {
    let root = tempfile::tempdir().unwrap();
    resume_fixture(root.path());
    write_jsonl(
        &paths(root.path())
            .claude
            .join("projects/project/claude-new.jsonl"),
        &[json!({"type":"custom-title","customTitle":"--agent"})],
    );
    let output = command(root.path())
        .args(["resume", "-a", "claude", "--", "--agent"])
        .env("PATH", root.path().join("bin"))
        .output()
        .unwrap();
    assert_resumed(&output, root.path(), "claude", "claude-new");
}

#[test]
fn resume_rejects_invalid_arguments_before_loading_sessions() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["resume"],
        vec!["resume", ""],
        vec!["resume", "--agent", "codex"],
        vec!["resume", "latest", "-a"],
        vec!["resume", "latest", "--agent"],
        vec!["resume", "latest", "--agent", ""],
        vec!["resume", "latest", "--agent", "unknown"],
        vec!["resume", "latest", "--unknown"],
        vec!["resume", "one", "two"],
        vec!["resume", "latest", "-a", "codex", "--agent", "pi"],
    ] {
        let output = command(root.path()).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Usage: agents-sessions resume"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn ls_query_matches_full_metadata_and_excludes_messages_agents_and_created_dates() {
    use chrono::{TimeZone, Utc};
    let root = tempfile::tempdir().unwrap();
    let data = paths(root.path());
    let created = Utc
        .with_ymd_and_hms(2025, 1, 1, 12, 0, 0)
        .unwrap()
        .timestamp_millis();
    let modified = Utc
        .with_ymd_and_hms(2026, 10, 24, 14, 30, 0)
        .unwrap()
        .timestamp_millis();
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"named","project":root.path(),"display":"prompt-only","timestamp":1000}),
            json!({"sessionId":"01234567-hidden-suffix","project":root.path(),"display":"prompt-only","timestamp":2000}),
            json!({"sessionId":"path","project":root.path().join("project-pattern"),"display":"prompt-only","timestamp":3000}),
            json!({"sessionId":"date","project":root.path(),"display":"prompt-only","timestamp":created}),
            json!({"sessionId":"date","project":root.path(),"display":"prompt-only","timestamp":modified}),
        ],
    );
    write_jsonl(
        &data.claude.join("projects/project/named.jsonl"),
        &[
            json!({"type":"custom-title","customTitle":"A long chat name with a hidden CAFÉ suffix"}),
        ],
    );
    for (query, expected) in [
        ("café", vec!["named"]),
        ("HIDDEN-SUFFIX", vec!["01234567"]),
        ("PROJECT-PATTERN", vec!["path"]),
        ("2026-10-24 14:30", vec!["date"]),
        ("10-2", vec!["date"]),
        ("prompt-only", vec![]),
        ("claude", vec![]),
        ("2025", vec![]),
    ] {
        let output = command(root.path())
            .args(["ls", "--all", query])
            .env("TZ", "UTC")
            .output()
            .unwrap();
        assert_eq!(listed_ids(&output), expected, "query: {query}");
        if expected.is_empty() {
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                "No sessions found.\n"
            );
        }
    }
}

#[test]
fn ls_query_scopes_names_to_cwd_and_supports_all_flags_before_or_after_the_query() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current");
    let child = current.join("child");
    let other = root.path().join("other");
    let alias = root.path().join("alias");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&current, &alias).unwrap();
    let data = paths(root.path());
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"cwd","project":current,"display":"hi","timestamp":4000}),
            json!({"sessionId":"child","project":child,"display":"hi","timestamp":3000}),
            json!({"sessionId":"other","project":other,"display":"hi","timestamp":2000}),
            json!({"sessionId":"alias","project":alias,"display":"hi","timestamp":1000}),
        ],
    );
    for id in ["cwd", "child", "other", "alias"] {
        write_jsonl(
            &data.claude.join(format!("projects/project/{id}.jsonl")),
            &[json!({"type":"custom-title","customTitle":"Needle chat"})],
        );
    }
    for (args, expected) in [
        (vec!["ls", "needle chat"], vec!["cwd", "alias"]),
        (
            vec!["ls", "-a", "needle chat"],
            vec!["cwd", "child", "other", "alias"],
        ),
        (
            vec!["ls", "--all", "needle chat"],
            vec!["cwd", "child", "other", "alias"],
        ),
        (
            vec!["ls", "needle chat", "-a"],
            vec!["cwd", "child", "other", "alias"],
        ),
        (
            vec!["ls", "needle chat", "--all"],
            vec!["cwd", "child", "other", "alias"],
        ),
    ] {
        let output = command(root.path())
            .args(args)
            .current_dir(&current)
            .output()
            .unwrap();
        assert_eq!(listed_ids(&output), expected);
    }
}

#[test]
fn ls_query_calendar_queries_return_the_same_matches_as_the_tui() {
    use agents_sessions::{data::agents::load_all_sessions, tui::State};
    use chrono::{Days, Local, Months};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let root = tempfile::tempdir().unwrap();
    let data = paths(root.path());
    // noon avoids ambiguous or missing clock times around DST changes
    let now = Local::now()
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .single()
        .unwrap();
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"current","project":root.path(),"display":"hi","timestamp":now.timestamp_millis()}),
            json!({"sessionId":"previous","project":root.path(),"display":"hi","timestamp":now.checked_sub_days(Days::new(1)).unwrap().timestamp_millis()}),
            json!({"sessionId":"older","project":root.path(),"display":"hi","timestamp":now.checked_sub_months(Months::new(12)).unwrap().timestamp_millis()}),
        ],
    );
    write_jsonl(
        &data.claude.join("projects/project/older.jsonl"),
        &[json!({"type":"custom-title","customTitle":"Today notes"})],
    );
    let sessions = load_all_sessions(&data).unwrap();
    for query in [
        "TODAY",
        "yesterday",
        "this week",
        "last week",
        "this month",
        "last month",
        "this year",
        "last year",
    ] {
        let mut state = State::new(sessions.clone(), root.path().to_path_buf());
        state.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        for character in query.chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let expected: Vec<_> = state
            .visible
            .iter()
            .map(|index| sessions[*index].session_id.clone())
            .collect();
        let output = command(root.path())
            .args(["ls", query])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(listed_ids(&output), expected, "query: {query}");
    }
}

#[test]
fn ls_query_is_optional_and_supports_empty_and_option_like_text() {
    let root = tempfile::tempdir().unwrap();
    for args in [vec!["ls", "--unknown", "query"], vec!["ls", "two", "words"]] {
        let output = command(root.path()).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: agents-sessions ls"));
    }
    let data = paths(root.path());
    write_jsonl(
        &data.claude.join("history.jsonl"),
        &[json!({"sessionId":"named","project":root.path(),"display":"hi","timestamp":1000})],
    );
    write_jsonl(
        &data.claude.join("projects/project/named.jsonl"),
        &[json!({"type":"custom-title","customTitle":"lookup -a flag"})],
    );
    for args in [
        vec!["ls"],
        vec!["ls", "-a"],
        vec!["ls", "--all"],
        vec!["ls", ""],
        vec!["ls", "--", "-a"],
        vec!["ls", "--all", "--", "-a"],
    ] {
        let output = command(root.path())
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(listed_ids(&output), vec!["named"]);
    }
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
        assert!(String::from_utf8_lossy(&output.stdout).contains("-a, --all"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("ls [QUERY]"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("resume <ID | NAME | latest>"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("-a, --agent"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("launch <ID>"));
        assert!(output.stderr.is_empty());
    }
    let output = command(root.path()).arg("delete").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a session ID argument"));
    for name in ["unknown", "find", "launch"] {
        let output = command(root.path()).arg(name).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&format!("Unknown command '{name}'"))
        );
    }
    for args in [vec!["ls", "--unknown"], vec!["ls", "one", "two"]] {
        let output = command(root.path()).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: agents-sessions ls"));
    }
    let output = command(root.path())
        .args(["ls", "--all", "unexpected", "extra"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("one query"));
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
        .args(["ls", "--all"])
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
fn listing_defaults_to_the_exact_current_directory_and_aliases() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current");
    let child = current.join("child");
    let other = root.path().join("other");
    let alias = root.path().join("current-alias");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&current, &alias).unwrap();
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"current-session","timestamp":4000,"project":current,"display":"current"}),
            json!({"sessionId":"child-session","timestamp":3000,"project":child,"display":"child"}),
            json!({"sessionId":"other-session","timestamp":2000,"project":other,"display":"other"}),
            json!({"sessionId":"alias-session","timestamp":1000,"project":alias,"display":"alias"}),
        ],
    );
    for (args, expected) in [
        (vec!["ls"], vec!["current-", "alias-se"]),
        (
            vec!["ls", "-a"],
            vec!["current-", "child-se", "other-se", "alias-se"],
        ),
        (
            vec!["ls", "--all"],
            vec!["current-", "child-se", "other-se", "alias-se"],
        ),
    ] {
        let output = command(root.path())
            .args(args)
            .current_dir(&current)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        let ids: Vec<_> = text
            .lines()
            .skip(1)
            .map(|line| line.split('\t').nth(1).unwrap().trim())
            .collect();
        assert_eq!(ids, expected);
    }
}

#[test]
fn listing_reports_empty_when_the_current_directory_has_no_sessions() {
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current");
    fs::create_dir(&current).unwrap();
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"elsewhere","timestamp":1000,"project":"/elsewhere","display":"other"}),
        ],
    );
    let output = command(root.path())
        .args(["ls"])
        .current_dir(current)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "No sessions found.\n"
    );
}

#[test]
fn resume_preserves_argument_boundaries_and_working_directory() {
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
        .args(["resume", "session"])
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
        .args(["resume", "missing"])
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
        .args(["resume", "id"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("no longer exists"));
    write_jsonl(
        &paths(root.path()).claude.join("history.jsonl"),
        &[json!({"sessionId":"id","project":root.path(),"display":"hi","timestamp":1000})],
    );
    let output = command(root.path())
        .args(["resume", "id"])
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
    for args in [vec!["ls", "--all"], vec!["delete", "tar"]] {
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
