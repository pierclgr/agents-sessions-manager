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
use std::{cell::RefCell, ffi::OsString, fs};

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
