//! session history, names, and lazy transcript details

use super::AgentProvider;
use crate::{
    data::{Agent, DataPaths, Session, delete},
    utils::{CommandRunner, children, is_within, visit_jsonl, visit_jsonl_matching},
};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs::File,
    io::{self, BufRead},
    path::{Path, PathBuf},
};

pub struct Claude;

const TITLE_MARKERS: [&str; 2] = ["\"custom-title\"", "\"ai-title\""];

struct HistoryEntry {
    timestamp: i64,
    display: String,
    project: String,
}

pub(crate) fn transcript(id: &str, root: &Path) -> Option<PathBuf> {
    if !matches!(
        Path::new(id).components().collect::<Vec<_>>().as_slice(),
        [std::path::Component::Normal(_)]
    ) {
        return None;
    }
    let projects = root.join("projects");
    children(&projects)
        .into_iter()
        .filter(|path| std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()))
        .map(|directory| directory.join(format!("{id}.jsonl")))
        .find(|path| path.is_file() && is_within(path, &projects))
}

impl AgentProvider for Claude {
    fn load_sessions(&self, paths: &DataPaths) -> Result<Vec<Session>, String> {
        let history = paths.claude.join("history.jsonl");
        let file = match File::open(&history) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return Err(format!(
                    "Error: Cannot read {}. Check permissions.",
                    history.display()
                ));
            }
            Err(error) => return Err(format!("Error: Cannot read {}: {error}", history.display())),
        };
        let mut groups: Vec<(String, Vec<HistoryEntry>)> = Vec::new();
        let mut indexes = HashMap::new();
        for (number, line) in io::BufReader::new(file).split(b'\n').enumerate() {
            let line =
                line.map_err(|error| format!("Error: Cannot read {}: {error}", history.display()))?;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value: Value = match serde_json::from_slice(&line) {
                Ok(value) => value,
                Err(_) => {
                    eprintln!(
                        "Warning: Skipping malformed JSON on line {} of {}",
                        number + 1,
                        history.display()
                    );
                    continue;
                }
            };
            let (Some(id), Some(timestamp), Some(display), Some(project)) = (
                value["sessionId"].as_str(),
                value["timestamp"].as_i64(),
                value["display"].as_str(),
                value["project"].as_str(),
            ) else {
                continue;
            };
            if id.is_empty()
                || timestamp == 0
                || display.is_empty()
                || project.is_empty()
                || Utc.timestamp_millis_opt(timestamp).single().is_none()
            {
                continue;
            }
            let index = *indexes.entry(id.to_owned()).or_insert_with(|| {
                groups.push((id.into(), Vec::new()));
                groups.len() - 1
            });
            groups[index].1.push(HistoryEntry {
                timestamp,
                display: display.into(),
                project: project.into(),
            });
        }
        let mut sessions = Vec::new();
        for (id, mut entries) in groups {
            entries.sort_by_key(|entry| entry.timestamp);
            let first = &entries[0];
            let last = entries.last().unwrap();
            let mut custom = None;
            let mut generated = None;
            if let Some(path) = transcript(&id, &paths.claude) {
                // avoid decoding full conversation messages when only the name is needed
                let _ = visit_jsonl_matching(&path, &TITLE_MARKERS, |entry| {
                    match entry["type"].as_str() {
                        Some("custom-title") => {
                            custom = entry["customTitle"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(String::from)
                        }
                        Some("ai-title") => {
                            generated = entry["aiTitle"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(String::from)
                        }
                        _ => {}
                    }
                });
            }
            sessions.push(Session {
                agent: Agent::Claude,
                session_id: id,
                project_path: first.project.clone().into(),
                created_at: Utc.timestamp_millis_opt(first.timestamp).unwrap(),
                last_active_at: Utc.timestamp_millis_opt(last.timestamp).unwrap(),
                latest_command: last.display.clone(),
                commands: entries.into_iter().map(|entry| entry.display).collect(),
                name: custom.or(generated),
                todos: vec![],
            });
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_active_at));
        Ok(sessions)
    }

    fn load_detail(&self, session: &mut Session, paths: &DataPaths) {
        let Some(path) = transcript(&session.session_id, &paths.claude) else {
            return;
        };
        let mut seen = HashSet::new();
        let mut todos = Vec::new();
        let _ = visit_jsonl(&path, |entry| {
            if let Some(items) = entry["todos"].as_array() {
                for item in items {
                    let text = match item {
                        Value::String(text) => text.clone(),
                        Value::Object(_) => ["content", "text", "title"]
                            .iter()
                            .filter_map(|key| item[key].as_str())
                            .find(|text| !text.is_empty())
                            .map(String::from)
                            .unwrap_or_else(|| item.to_string()),
                        _ => continue,
                    };
                    if seen.insert(text.clone()) {
                        todos.push(text);
                    }
                }
            }
        });
        session.todos = todos;
    }

    fn delete_sessions(
        &self,
        sessions: &[&Session],
        paths: &DataPaths,
        runner: &dyn CommandRunner,
    ) -> Vec<String> {
        delete::claude_sessions(sessions, &paths.claude, runner)
    }

    fn resume_command(&self, session: &Session, _paths: &DataPaths) -> Vec<OsString> {
        vec![
            "claude".into(),
            "--resume".into(),
            session.session_id.clone().into(),
        ]
    }
}
