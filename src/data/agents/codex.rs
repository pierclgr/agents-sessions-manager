//! read-only thread discovery and CLI-owned deletion

use super::{AgentProvider, SessionLoad};
use crate::{
    data::{Agent, DataPaths, Session},
    utils::{CommandRunner, children, is_within, open_readonly, remove_jsonl_lines, visit_jsonl},
};
use chrono::{TimeZone, Utc};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
};

pub struct Codex;

fn read_sessions(paths: &DataPaths, load: SessionLoad<'_>) -> Result<Vec<Session>, String> {
    let db = children(&paths.codex)
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            let version = name.strip_prefix("state_")?.strip_suffix(".sqlite")?;
            if !version.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let version = version.parse::<u64>().ok()?;
            Some((version, path))
        })
        .max_by_key(|(version, _)| *version);
    let Some((_, db)) = db else {
        return Ok(vec![]);
    };
    let result = (|| -> rusqlite::Result<Vec<Session>> {
        let connection = open_readonly(&db)?;
        let mut query = connection.prepare("SELECT id, cwd, name, first_user_message, created_at_ms, updated_at_ms FROM threads WHERE archived = 0 AND thread_source IS NOT 'subagent' AND first_user_message != '' AND (?1 IS NULL OR instr(id, ?1) = 1)")?;
        let mut sessions = Vec::new();
        let mut rows = query.query([load.id_prefix()])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let first_message: String = row.get(3)?;
            let (Some(created), Some(updated)) = (
                Utc.timestamp_millis_opt(row.get(4)?).single(),
                Utc.timestamp_millis_opt(row.get(5)?).single(),
            ) else {
                continue;
            };
            sessions.push(Session {
                agent: Agent::Codex,
                session_id: id,
                project_path: row.get::<_, String>(1)?.into(),
                name: row
                    .get::<_, Option<String>>(2)?
                    .filter(|name| !name.is_empty()),
                created_at: created,
                last_active_at: updated,
                latest_command: String::new(),
                commands: vec![first_message],
                todos: vec![],
            });
        }
        if sessions.is_empty() {
            return Ok(sessions);
        }
        let eligible_ids: HashSet<_> = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        let mut prompts: HashMap<String, Vec<String>> = HashMap::new();
        let _ = visit_jsonl(&paths.codex.join("history.jsonl"), |entry| {
            let (Some(id), Some(text)) = (entry["session_id"].as_str(), entry["text"].as_str())
            else {
                return;
            };
            if id.is_empty() || text.is_empty() || !eligible_ids.contains(id) {
                return;
            }
            load.push_command(prompts.entry(id.into()).or_default(), text.into());
        });
        for session in &mut sessions {
            if let Some(commands) = prompts.remove(&session.session_id) {
                session.commands = commands;
            }
            session.latest_command = session.commands.last().cloned().unwrap_or_default();
        }
        Ok(sessions)
    })();
    match result {
        Ok(sessions) => Ok(sessions),
        Err(error) => {
            eprintln!(
                "Warning: Cannot read Codex sessions from {}: {error}",
                db.display()
            );
            Ok(vec![])
        }
    }
}

impl AgentProvider for Codex {
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
        paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        let mut deleted = HashSet::new();
        for session in sessions {
            match runner.run(&[
                "codex".into(),
                "delete".into(),
                "--force".into(),
                session.session_id.clone().into(),
            ]) {
                Ok(()) => {
                    deleted.insert(session.session_id.clone());
                }
                Err(error) => errors.push(error),
            }
        }
        if !deleted.is_empty() {
            let history = paths.codex.join("history.jsonl");
            if history.exists() && !is_within(&history, &paths.codex) {
                errors.push(format!(
                    "Could not rewrite {}: path is outside the data directory",
                    history.display()
                ));
            } else if let Err(error) = remove_jsonl_lines(&history, "session_id", &deleted) {
                errors.push(format!("Could not rewrite {}: {error}", history.display()));
            }
        }
        errors
    }

    fn resume_command(&self, session: &Session, _paths: &DataPaths) -> Vec<OsString> {
        vec![
            "codex".into(),
            "resume".into(),
            session.session_id.clone().into(),
        ]
    }
}
