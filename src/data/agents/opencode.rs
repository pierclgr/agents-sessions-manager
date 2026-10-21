//! top-level sessions and user messages from the v2 database

use super::{AgentProvider, SessionLoad};
use crate::{
    data::{Agent, DataPaths, Session},
    utils::{CommandRunner, open_readonly},
};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::{collections::HashMap, ffi::OsString};

pub struct OpenCode;

fn read_sessions(paths: &DataPaths, load: SessionLoad<'_>) -> Result<Vec<Session>, String> {
    if !paths.opencode_db.is_file() {
        return Ok(vec![]);
    }
    let result = (|| -> rusqlite::Result<Vec<Session>> {
        let connection = open_readonly(&paths.opencode_db)?;
        let mut query = connection.prepare("SELECT session_id, data FROM session_message WHERE type = 'user' AND session_id IN (SELECT id FROM session_v2 WHERE parent_id IS NULL AND time_archived IS NULL AND (?1 IS NULL OR instr(id, ?1) = 1)) ORDER BY session_id, seq")?;
        let mut rows = query.query([load.id_prefix()])?;
        let mut prompts: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let data: String = row.get(1)?;
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            if let Some(text) = value["text"].as_str().filter(|text| !text.is_empty()) {
                load.push_command(prompts.entry(id).or_default(), text.into());
            }
        }
        let mut query = connection.prepare("SELECT id, directory, title, time_created, time_updated FROM session_v2 WHERE parent_id IS NULL AND time_archived IS NULL AND (?1 IS NULL OR instr(id, ?1) = 1)")?;
        let mut rows = query.query([load.id_prefix()])?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let Some(commands) = prompts.remove(&id) else {
                continue;
            };
            let (Some(created), Some(updated)) = (
                Utc.timestamp_millis_opt(row.get(3)?).single(),
                Utc.timestamp_millis_opt(row.get(4)?).single(),
            ) else {
                continue;
            };
            sessions.push(Session {
                agent: Agent::OpenCode,
                session_id: id,
                project_path: row.get::<_, String>(1)?.into(),
                name: row
                    .get::<_, Option<String>>(2)?
                    .filter(|title| !title.is_empty() && !title.starts_with("New session - ")),
                created_at: created,
                last_active_at: updated,
                latest_command: commands.last().cloned().unwrap_or_default(),
                commands,
                todos: vec![],
            });
        }
        Ok(sessions)
    })();
    match result {
        Ok(sessions) => Ok(sessions),
        Err(error) => {
            eprintln!(
                "Warning: Cannot read OpenCode sessions from {}: {error}",
                paths.opencode_db.display()
            );
            Ok(vec![])
        }
    }
}

impl AgentProvider for OpenCode {
    fn load_sessions(&self, paths: &DataPaths) -> Result<Vec<Session>, String> {
        read_sessions(paths, SessionLoad::Full)
    }

    fn load_summaries(
        &self,
        paths: &DataPaths,
        id_prefix: Option<&str>,
    ) -> Result<Vec<Session>, String> {
        read_sessions(paths, SessionLoad::Recent(id_prefix))
    }

    fn delete_sessions(
        &self,
        sessions: &[&Session],
        _paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Vec<String> {
        sessions
            .iter()
            .filter_map(|session| {
                runner
                    .run(&[
                        "opencode".into(),
                        "session".into(),
                        "delete".into(),
                        session.session_id.clone().into(),
                    ])
                    .err()
            })
            .collect()
    }

    fn resume_command(&self, session: &Session, _paths: &DataPaths) -> Vec<OsString> {
        vec![
            "opencode".into(),
            "--session".into(),
            session.session_id.clone().into(),
        ]
    }
}
