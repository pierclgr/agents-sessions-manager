mod common;

use agents_sessions::{
    data::{
        Agent,
        agents::{load_all_sessions, provider},
    },
    utils::CommandRunner,
};
use common::{paths, session, write_jsonl};
use rusqlite::Connection;
use serde_json::json;
use std::{
    cell::RefCell,
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[derive(Default)]
struct Runner {
    calls: RefCell<Vec<Vec<OsString>>>,
    fail: bool,
}

impl CommandRunner for Runner {
    fn run(&self, argv: &[OsString]) -> Result<(), String> {
        self.calls.borrow_mut().push(argv.to_vec());
        if self.fail {
            Err("unpushed commits".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn claude_groups_sorts_titles_and_loads_todos_lazily() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"a", "timestamp":2000,"display":"second","project":"/p/my_proj.x"}),
            json!({"sessionId":"a", "timestamp":1000,"display":"first","project":"/p/my_proj.x"}),
            json!({"sessionId":"b", "timestamp":3000,"display":"other","project":"/p/b"}),
            json!({"sessionId":"empty"}),
            json!([]),
        ],
    );
    write_jsonl(
        &paths.claude.join("projects/-p-my-proj-x/a.jsonl"),
        &[
            json!({"type":"custom-title", "customTitle":"old"}),
            json!({"type":"custom-title", "customTitle":"new-name"}),
            json!({"type":"ai-title", "aiTitle":"generated"}),
            json!({"todos":[{"content":"write tests"},"write tests",{"text":"check"}]}),
        ],
    );
    write_jsonl(
        &paths.claude.join("projects/-p-b/b.jsonl"),
        &[
            json!({"type":"ai-title","aiTitle":"old"}),
            json!({"type":"ai-title","aiTitle":"Auto title"}),
        ],
    );
    let mut sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert_eq!(sessions[0].session_id, "b");
    assert_eq!(sessions[0].name.as_deref(), Some("Auto title"));
    let named = &mut sessions[1];
    assert_eq!(named.name.as_deref(), Some("new-name"));
    assert_eq!(named.commands, ["first", "second"]);
    assert_eq!(named.latest_command, "second");
    assert_eq!(named.created_at.timestamp_millis(), 1000);
    assert!(named.todos.is_empty());
    provider(Agent::Claude).load_detail(named, &paths);
    assert_eq!(named.todos, ["write tests", "check"]);
}

#[test]
fn claude_title_lookup_keeps_project_order_and_filesystem_case_matching() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let ids = ["duplicate", "mixed", "upper-extension"];
    let history: Vec<_> = ids
        .iter()
        .map(|id| json!({"sessionId":id,"timestamp":1000,"display":"hi","project":"/p"}))
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    for project in ["z-last", "a-first"] {
        for filename in ["duplicate.jsonl", "mixed.jsonl"] {
            if project == "a-first" && filename == "mixed.jsonl" {
                continue;
            }
            write_jsonl(
                &paths.claude.join("projects").join(project).join(filename),
                &[json!({"type":"custom-title","customTitle":project})],
            );
        }
    }
    let mixed = paths.claude.join("projects/a-first/MiXeD.jsonl");
    write_jsonl(
        &mixed,
        &[json!({"type":"custom-title","customTitle":"case alias"})],
    );
    write_jsonl(
        &paths.claude.join("projects/a-first/upper-extension.JSONL"),
        &[json!({"type":"custom-title","customTitle":"upper extension"})],
    );
    let sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), ids.len());
    assert_eq!(sessions[0].name.as_deref(), Some("a-first"));
    let expected_mixed = if mixed.with_file_name("mixed.jsonl").is_file() {
        "case alias"
    } else {
        "z-last"
    };
    assert_eq!(sessions[1].name.as_deref(), Some(expected_mixed));
    let expected_extension = paths
        .claude
        .join("projects/a-first/upper-extension.jsonl")
        .is_file()
        .then_some("upper extension");
    assert_eq!(sessions[2].name.as_deref(), expected_extension);
}

#[test]
fn claude_title_lookup_keeps_missing_and_unusual_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let cases = [
        ("missing", None),
        ("../escape", None),
        ("nested/child", None),
        (".", None),
        ("..", None),
        ("odd/", Some("trailing slash")),
        ("trailing/.", Some("trailing dot")),
    ];
    let history: Vec<_> = cases
        .iter()
        .map(|(id, _)| json!({"sessionId":id,"timestamp":1000,"display":"hi","project":"/p"}))
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    for (filename, title) in [
        ("escape.jsonl", "rejected parent"),
        ("project/nested/child.jsonl", "rejected nested path"),
        ("project/..jsonl", "rejected dot"),
        ("project/...jsonl", "rejected parent component"),
        ("project/odd/.jsonl", "trailing slash"),
        ("project/trailing/..jsonl", "trailing dot"),
    ] {
        write_jsonl(
            &paths.claude.join("projects").join(filename),
            &[json!({"type":"custom-title","customTitle":title})],
        );
    }
    let sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), cases.len());
    for (session, (id, name)) in sessions.iter().zip(cases) {
        assert_eq!(session.session_id, id);
        assert_eq!(session.name.as_deref(), name, "session ID {id:?}");
    }
}

#[test]
fn claude_title_lookup_keeps_unicode_filesystem_aliases() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let cases = [("K", "K"), ("é", "e\u{301}"), ("日本語", "日本語")];
    let history: Vec<_> = cases
        .iter()
        .map(|(id, _)| json!({"sessionId":id,"timestamp":1000,"display":"hi","project":"/p"}))
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    for (id, alias) in cases {
        for (project, filename) in [("a-first", alias), ("z-last", id)] {
            write_jsonl(
                &paths
                    .claude
                    .join("projects")
                    .join(project)
                    .join(format!("{filename}.jsonl")),
                &[json!({"type":"custom-title","customTitle":project})],
            );
        }
    }
    let sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), cases.len());
    for (session, (id, _)) in sessions.iter().zip(cases) {
        let expected = if paths
            .claude
            .join("projects/a-first")
            .join(format!("{id}.jsonl"))
            .is_file()
        {
            "a-first"
        } else {
            "z-last"
        };
        assert_eq!(session.name.as_deref(), Some(expected), "session ID {id:?}");
    }
}

#[test]
fn claude_title_lookup_respects_project_and_transcript_links() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let projects = paths.claude.join("projects");
    let ids = ["duplicate", "outside", "inside", "directory"];
    let history: Vec<_> = ids
        .iter()
        .map(|id| json!({"sessionId":id,"timestamp":1000,"display":"hi","project":"/p"}))
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    for (filename, title) in [
        ("a-first/duplicate.jsonl", "first real project"),
        ("z-last/duplicate.jsonl", "last real project"),
        ("z-last/outside.jsonl", "safe fallback"),
        ("z-last/target.txt", "internal link"),
    ] {
        write_jsonl(
            &projects.join(filename),
            &[json!({"type":"custom-title","customTitle":title})],
        );
    }
    write_jsonl(
        &outside.path().join("outside.jsonl"),
        &[json!({"type":"custom-title","customTitle":"outside"})],
    );
    symlink(projects.join("z-last"), projects.join("00-linked-project")).unwrap();
    symlink(
        outside.path().join("outside.jsonl"),
        projects.join("a-first/outside.jsonl"),
    )
    .unwrap();
    symlink(
        projects.join("z-last/target.txt"),
        projects.join("a-first/inside.jsonl"),
    )
    .unwrap();
    fs::create_dir(projects.join("a-first/directory.jsonl")).unwrap();
    let sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
    let names: Vec<_> = sessions
        .iter()
        .map(|session| session.name.as_deref())
        .collect();
    assert_eq!(
        names,
        [
            Some("first real project"),
            Some("safe fallback"),
            Some("internal link"),
            None,
        ]
    );
}

#[test]
fn claude_title_lookup_works_without_project_directory_listing_permission() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[json!({"sessionId":"duplicate","timestamp":1000,"display":"hi","project":"/p"})],
    );
    for project in ["a-first", "z-last"] {
        write_jsonl(
            &paths
                .claude
                .join("projects")
                .join(project)
                .join("duplicate.jsonl"),
            &[json!({"type":"custom-title","customTitle":project})],
        );
    }
    let first_project = paths.claude.join("projects/a-first");
    let permissions = fs::metadata(&first_project).unwrap().permissions();
    fs::set_permissions(&first_project, fs::Permissions::from_mode(0o111)).unwrap();
    let result = provider(Agent::Claude).load_sessions(&paths);
    fs::set_permissions(&first_project, permissions).unwrap();
    let sessions = result.unwrap();
    assert_eq!(sessions[0].name.as_deref(), Some("a-first"));
}

#[test]
fn claude_title_lookup_refreshes_between_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[json!({"sessionId":"id","timestamp":1000,"display":"hi","project":"/p"})],
    );
    let first = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert!(first[0].name.is_none());
    let transcript = paths.claude.join("projects/project/id.jsonl");
    for title in ["created", "renamed"] {
        write_jsonl(
            &transcript,
            &[json!({"type":"custom-title","customTitle":title})],
        );
        let sessions = provider(Agent::Claude).load_sessions(&paths).unwrap();
        assert_eq!(sessions[0].name.as_deref(), Some(title));
    }
    fs::remove_file(transcript).unwrap();
    let last = provider(Agent::Claude).load_sessions(&paths).unwrap();
    assert!(last[0].name.is_none());
}

#[test]
fn all_providers_handle_missing_data() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    for agent in Agent::ALL {
        assert!(provider(agent).load_sessions(&paths).unwrap().is_empty());
    }
}

fn codex_db(path: &std::path::Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (id TEXT, cwd TEXT, name TEXT, first_user_message TEXT,
        created_at_ms INTEGER, updated_at_ms INTEGER, archived INTEGER, thread_source TEXT);
        INSERT INTO threads VALUES
        ('named','/p/a','my-thread','first',1000,9000,0,'user'),
        ('plain','/p/b',NULL,'only prompt',2000,8000,0,NULL),
        ('sub','/p/a',NULL,'sub',1,1,0,'subagent'),
        ('archived','/p/a',NULL,'old',1,1,1,'user'),
        ('empty','/p/a',NULL,'',1,1,0,'user');",
    )
    .unwrap();
}

#[test]
fn codex_uses_newest_numeric_database_and_history_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    codex_db(&paths.codex.join("state_10.sqlite"));
    fs::write(paths.codex.join("state_9.sqlite"), "not a database").unwrap();
    fs::write(paths.codex.join("state_invalid.sqlite"), "ignored").unwrap();
    write_jsonl(
        &paths.codex.join("history.jsonl"),
        &[
            json!({"session_id":"named","text":"first"}),
            json!({"session_id":"named","text":"second"}),
            json!([]),
        ],
    );
    let sessions = provider(Agent::Codex).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name.as_deref(), Some("my-thread"));
    assert_eq!(sessions[0].commands, ["first", "second"]);
    assert_eq!(sessions[0].last_active_at.timestamp(), 9);
    assert_eq!(sessions[1].commands, ["only prompt"]);
    assert!(sessions[1].name.is_none());
}

#[test]
fn codex_cleans_history_only_after_successful_cli_delete() {
    for fail in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths(tmp.path());
        write_jsonl(
            &paths.codex.join("history.jsonl"),
            &[
                json!({"session_id":"named","text":"delete"}),
                json!({"session_id":"other","text":"keep"}),
            ],
        );
        let runner = Runner {
            fail,
            ..Default::default()
        };
        let errors = provider(Agent::Codex).delete_sessions(
            &[&session(Agent::Codex, "named")],
            &paths,
            &runner,
        );
        assert_eq!(errors.len(), usize::from(fail));
        assert_eq!(
            runner.calls.borrow()[0],
            ["codex", "delete", "--force", "named"].map(OsString::from)
        );
        let history = fs::read_to_string(paths.codex.join("history.jsonl")).unwrap();
        assert_eq!(history.contains("named"), fail);
        assert!(history.contains("other"));
    }
}

#[test]
fn opencode_filters_and_orders_prompts() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    fs::create_dir_all(paths.opencode_db.parent().unwrap()).unwrap();
    let db = Connection::open(&paths.opencode_db).unwrap();
    db.execute_batch(
        "CREATE TABLE session_v2 (id TEXT, directory TEXT, title TEXT, time_created INTEGER,
        time_updated INTEGER, parent_id TEXT, time_archived INTEGER);
        CREATE TABLE session_message (session_id TEXT, data TEXT, type TEXT, seq INTEGER);
        INSERT INTO session_v2 VALUES
        ('named','/p/a','My work',1000,9000,NULL,NULL),
        ('default','/p/b','New session - date',2000,8000,NULL,NULL),
        ('child','/p/a','Child',1,1,'named',NULL),
        ('archived','/p/a','Old',1,1,NULL,5000),
        ('empty','/p/a','Empty',1,1,NULL,NULL);
        INSERT INTO session_message VALUES
        ('named','{\"text\":\"second\"}','user',2),
        ('named','{\"text\":\"first\"}','user',1),
        ('named','{\"text\":\"reply\"}','assistant',3),
        ('default','{\"text\":\"hello\"}','user',1),
        ('child','{\"text\":\"hidden\"}','user',1),
        ('archived','{\"text\":\"hidden\"}','user',1);",
    )
    .unwrap();
    let sessions = provider(Agent::OpenCode).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name.as_deref(), Some("My work"));
    assert_eq!(sessions[0].commands, ["first", "second"]);
    assert_eq!(sessions[0].created_at.timestamp(), 1);
    assert!(sessions[1].name.is_none());
}

#[test]
fn opencode_delete_reports_command_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    for fail in [false, true] {
        let runner = Runner {
            fail,
            ..Default::default()
        };
        let errors = provider(Agent::OpenCode).delete_sessions(
            &[&session(Agent::OpenCode, "id")],
            &paths,
            &runner,
        );
        assert_eq!(errors.len(), usize::from(fail));
        assert_eq!(
            runner.calls.borrow()[0],
            ["opencode", "session", "delete", "id"].map(OsString::from)
        );
    }
}

#[test]
fn pi_reads_header_messages_names_and_last_activity() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    write_jsonl(
        &paths.pi_sessions.join("--p-a--/date_named.jsonl"),
        &[
            json!({"type":"session","id":"named","cwd":"/p/a","timestamp":"2026-09-07T14:15:20.761Z"}),
            json!({"type":"message","message":{"role":"user","content":[{"type":"text","text":"first"}]}}),
            json!({"type":"message","message":{"role":"assistant","content":"reply"}}),
            json!({"type":"session_info","name":"old"}),
            json!({"type":"session_info","name":"new name"}),
            json!({"type":"message","timestamp":"2026-09-07T14:18:00Z","message":{"role":"user","content":"second"}}),
        ],
    );
    write_jsonl(
        &paths.pi_sessions.join("--p-b--/date_empty.jsonl"),
        &[json!({"type":"session","id":"empty","cwd":"/p/b","timestamp":"2026-09-07T14:15:20Z"})],
    );
    write_jsonl(
        &paths.pi_sessions.join("--p-b--/broken.jsonl"),
        &[json!({"type":"message"})],
    );
    let sessions = provider(Agent::Pi).load_sessions(&paths).unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.name.as_deref(), Some("new name"));
    assert_eq!(s.commands, ["first", "second"]);
    assert_eq!(s.created_at.to_rfc3339(), "2026-09-07T14:15:20.761+00:00");
    assert_eq!(s.last_active_at.to_rfc3339(), "2026-09-07T14:18:00+00:00");
    let resume = provider(Agent::Pi).resume_command(s, &paths);
    assert_eq!(
        resume[2],
        paths.pi_sessions.join("--p-a--/date_named.jsonl")
    );
    assert!(
        provider(Agent::Pi)
            .delete_sessions(&[s], &paths, &Runner::default())
            .is_empty()
    );
    assert!(!paths.pi_sessions.join("--p-a--").exists());
    assert_eq!(
        provider(Agent::Pi)
            .delete_sessions(&[s], &paths, &Runner::default())
            .len(),
        1
    );
    assert_eq!(provider(Agent::Pi).resume_command(s, &paths)[2], "named");
}

#[test]
fn resume_commands_preserve_agent_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    for (agent, expected) in [
        (Agent::Claude, vec!["claude", "--resume", "id"]),
        (Agent::Codex, vec!["codex", "resume", "id"]),
        (Agent::OpenCode, vec!["opencode", "--session", "id"]),
        (Agent::Pi, vec!["pi", "--session", "id"]),
    ] {
        assert_eq!(
            provider(agent).resume_command(&session(agent, "id"), &paths),
            expected.iter().map(OsString::from).collect::<Vec<_>>()
        );
    }
}

#[test]
fn aggregate_is_sorted_across_agents_with_stable_ties() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    codex_db(&paths.codex.join("state_1.sqlite"));
    write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[json!({"sessionId":"c","display":"hi","project":"/p","timestamp":9000})],
    );
    let sessions = load_all_sessions(&paths).unwrap();
    assert_eq!(
        sessions.iter().map(|s| s.agent).collect::<Vec<_>>(),
        [Agent::Claude, Agent::Codex, Agent::Codex]
    );
}

#[test]
fn sqlite_access_is_readonly_and_never_creates_a_missing_database() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("db.sqlite");
    assert!(agents_sessions::utils::open_readonly(&db).is_err());
    assert!(!db.exists());
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE example (id TEXT)")
        .unwrap();
    let readonly = agents_sessions::utils::open_readonly(&db).unwrap();
    assert!(
        readonly
            .execute("INSERT INTO example VALUES ('no')", [])
            .is_err()
    );
}

fn summary_fixture(root: &std::path::Path) -> agents_sessions::data::DataPaths {
    let paths = paths(root);
    let order = [7, 2, 0, 5, 4, 3, 1, 6];
    let history: Vec<_> = order
        .iter()
        .map(|number| {
            json!({
                "sessionId":"c", "timestamp":(number + 1) * 1000,
                "display":format!("prompt-{number}"),
                "project":if *number == 0 { "/first" } else { "/later" }
            })
        })
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    write_jsonl(
        &paths.claude.join("projects/project/c.jsonl"),
        &[
            json!({"type":"custom-title","customTitle":"Claude title"}),
            json!({"todos":["lazy todo"]}),
        ],
    );
    codex_db(&paths.codex.join("state_1.sqlite"));
    let codex = Connection::open(paths.codex.join("state_1.sqlite")).unwrap();
    codex
        .execute_batch(
            "INSERT INTO threads VALUES
        ('literal%id','/p',NULL,'percent',1000,9000,0,'user'),
        ('literal_other','/p',NULL,'underscore',1000,9000,0,'user');",
        )
        .unwrap();
    let codex_history: Vec<_> = (0..8)
        .map(|number| {
            json!({
                "session_id":"named","text":format!("prompt-{number}")
            })
        })
        .collect();
    write_jsonl(&paths.codex.join("history.jsonl"), &codex_history);
    fs::create_dir_all(paths.opencode_db.parent().unwrap()).unwrap();
    let opencode = Connection::open(&paths.opencode_db).unwrap();
    opencode
        .execute_batch(
            "CREATE TABLE session_v2 (id TEXT, directory TEXT, title TEXT,
        time_created INTEGER,time_updated INTEGER,parent_id TEXT,time_archived INTEGER);
        CREATE TABLE session_message (session_id TEXT,data TEXT,type TEXT,seq INTEGER);
        INSERT INTO session_v2 VALUES ('o','/p','OpenCode title',1000,9000,NULL,NULL);",
        )
        .unwrap();
    for number in order {
        opencode
            .execute(
                "INSERT INTO session_message VALUES ('o',?1,'user',?2)",
                rusqlite::params![
                    json!({"text":format!("prompt-{number}")}).to_string(),
                    number
                ],
            )
            .unwrap();
    }
    let mut pi =
        vec![json!({"type":"session","id":"pi","cwd":"/p", "timestamp":"1970-01-01T00:00:01Z"})];
    for number in 0..8 {
        pi.push(
            json!({"type":"message","timestamp":format!("1970-01-01T00:00:{:02}Z",number+2),
            "message":{"role":"user","content":format!("prompt-{number}")}}),
        );
    }
    pi.push(json!({"type":"session_info","name":"Pi title"}));
    write_jsonl(&paths.pi_sessions.join("project/date_pi.jsonl"), &pi);
    paths
}

#[test]
fn summaries_keep_metadata_order_and_only_five_recent_prompts() {
    use agents_sessions::data::agents::{load_session_names, load_session_summaries};
    let root = tempfile::tempdir().unwrap();
    let paths = summary_fixture(root.path());
    let full = load_all_sessions(&paths).unwrap();
    let mut recent = load_session_summaries(&paths, None).unwrap();
    assert_eq!(
        recent.iter().map(|s| s.key()).collect::<Vec<_>>(),
        full.iter().map(|s| s.key()).collect::<Vec<_>>()
    );
    for id in ["c", "named", "o", "pi"] {
        let complete = full.iter().find(|s| s.session_id == id).unwrap();
        let summary = recent.iter().find(|s| s.session_id == id).unwrap();
        assert_eq!(
            complete.commands,
            (0..8).map(|n| format!("prompt-{n}")).collect::<Vec<_>>()
        );
        assert_eq!(
            summary.commands,
            (3..8).map(|n| format!("prompt-{n}")).collect::<Vec<_>>()
        );
        assert_eq!(summary.latest_command, "prompt-7");
        assert_eq!(summary.created_at, complete.created_at);
        assert_eq!(summary.last_active_at, complete.last_active_at);
        assert_eq!(summary.project_path, complete.project_path);
        assert!(summary.todos.is_empty());
    }
    let claude = recent.iter().find(|s| s.agent == Agent::Claude).unwrap();
    assert_eq!(claude.project_path, std::path::Path::new("/first"));
    assert!(claude.name.is_none());
    let indices: Vec<_> = (0..recent.len()).collect();
    load_session_names(&mut recent, &indices, &paths);
    for (summary, complete) in recent.iter().zip(&full) {
        assert_eq!(summary.name, complete.name);
    }
    let claude = recent
        .iter_mut()
        .find(|s| s.agent == Agent::Claude)
        .unwrap();
    provider(Agent::Claude).load_detail(claude, &paths);
    assert_eq!(claude.todos, ["lazy todo"]);
}

#[test]
fn summary_prefixes_are_literal_and_keep_global_activity_order() {
    use agents_sessions::data::agents::load_session_summaries;
    let root = tempfile::tempdir().unwrap();
    let paths = summary_fixture(root.path());
    let full = load_all_sessions(&paths).unwrap();
    for prefix in [
        "", "c", "named", "NAMED", "literal%", "literal_", "o", "pi", "missing",
    ] {
        let selected = load_session_summaries(&paths, Some(prefix)).unwrap();
        let expected: Vec<_> = full
            .iter()
            .filter(|s| s.session_id.starts_with(prefix))
            .map(|s| s.key())
            .collect();
        assert_eq!(
            selected.iter().map(|s| s.key()).collect::<Vec<_>>(),
            expected,
            "prefix {prefix:?}"
        );
    }
}

#[test]
fn claude_summary_keeps_input_order_for_equal_timestamps() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    let history: Vec<_> = (0..7)
        .map(|number| {
            json!({
                "sessionId":"id", "timestamp":1000,
                "display":format!("prompt-{number}"), "project":format!("/project-{number}")
            })
        })
        .collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    let sessions = provider(Agent::Claude)
        .load_summaries(&paths, None)
        .unwrap();
    assert_eq!(sessions[0].project_path, std::path::Path::new("/project-0"));
    assert_eq!(sessions[0].latest_command, "prompt-6");
    assert_eq!(
        sessions[0].commands,
        (2..7)
            .map(|number| format!("prompt-{number}"))
            .collect::<Vec<_>>()
    );
}
