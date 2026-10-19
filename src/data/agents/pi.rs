//! one session per transcript file

use super::AgentProvider;
use crate::{
    data::{Agent, DataPaths, Session},
    utils::{CommandRunner, is_within, parse_timestamp, project_files, visit_jsonl},
};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub struct Pi;

fn session_file(id: &str, root: &Path) -> Option<PathBuf> {
    let suffix = format!("_{id}.jsonl");
    project_files(root, "jsonl").into_iter().find(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
    })
}

fn parse_file(path: &Path) -> Option<Session> {
    let mut header: Option<Value> = None;
    let mut commands = Vec::new();
    let mut name = None;
    let mut last_timestamp = None;
    let mut invalid_header = false;
    visit_jsonl(path, |entry| {
        if header.is_none() {
            invalid_header = entry["type"] != "session";
            header = Some(entry);
            return;
        }
        if let Some(timestamp) = entry["timestamp"].as_str() {
            last_timestamp = Some(timestamp.to_owned());
        }
        if entry["type"] == "session_info" {
            name = entry["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .map(String::from);
        } else if entry["type"] == "message" && entry["message"]["role"] == "user" {
            let content = &entry["message"]["content"];
            let text = match content {
                Value::String(text) => text.clone(),
                Value::Array(blocks) => blocks
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            };
            if !text.is_empty() {
                commands.push(text);
            }
        }
    })
    .ok()?;
    if invalid_header || commands.is_empty() {
        return None;
    }
    let header = header?;
    let created_at = parse_timestamp(header["timestamp"].as_str()?)?;
    let last_active_at = match last_timestamp {
        Some(timestamp) => parse_timestamp(&timestamp)?,
        None => created_at,
    };
    Some(Session {
        agent: Agent::Pi,
        session_id: header["id"].as_str()?.into(),
        project_path: header["cwd"].as_str()?.into(),
        created_at,
        last_active_at,
        latest_command: commands.last()?.clone(),
        commands,
        name,
        todos: vec![],
    })
}

impl AgentProvider for Pi {
    fn load_sessions(&self, paths: &DataPaths) -> Result<Vec<Session>, String> {
        Ok(project_files(&paths.pi_sessions, "jsonl")
            .iter()
            .filter_map(|path| parse_file(path))
            .collect())
    }

    fn delete_sessions(
        &self,
        sessions: &[&Session],
        paths: &DataPaths,
        _runner: &dyn CommandRunner,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for session in sessions {
            let Some(path) = session_file(&session.session_id, &paths.pi_sessions) else {
                errors.push(format!(
                    "Pi session file not found for {}",
                    session.session_id
                ));
                continue;
            };
            if !is_within(&path, &paths.pi_sessions) {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => {
                    let _ = fs::remove_dir(path.parent().unwrap());
                }
                Err(error) => errors.push(format!("Could not delete {}: {error}", path.display())),
            }
        }
        errors
    }

    fn resume_command(&self, session: &Session, paths: &DataPaths) -> Vec<OsString> {
        let target = session_file(&session.session_id, &paths.pi_sessions)
            .map(PathBuf::into_os_string)
            .unwrap_or_else(|| session.session_id.clone().into());
        vec!["pi".into(), "--session".into(), target]
    }
}
