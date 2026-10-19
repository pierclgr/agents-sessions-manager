#![allow(dead_code)]

use agents_sessions::data::{Agent, DataPaths, Session};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::{fs, path::Path};

pub fn paths(root: &Path) -> DataPaths {
    DataPaths {
        claude: root.join("claude"),
        codex: root.join("codex"),
        opencode_db: root.join("data/opencode/opencode.db"),
        pi_sessions: root.join("pi/sessions"),
    }
}

pub fn write_jsonl(path: &Path, entries: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = entries
        .iter()
        .map(|entry| format!("{entry}\n"))
        .collect::<String>();
    fs::write(path, text).unwrap();
}

pub fn session(agent: Agent, id: &str) -> Session {
    Session {
        agent,
        session_id: id.into(),
        project_path: "/p/proj".into(),
        created_at: Utc.timestamp_millis_opt(1000).unwrap(),
        last_active_at: Utc.timestamp_millis_opt(2000).unwrap(),
        latest_command: "hello".into(),
        commands: vec!["hello".into()],
        name: None,
        todos: vec![],
    }
}
