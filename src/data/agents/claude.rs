//! session history, names, and lazy transcript details

use super::{AgentProvider, SessionLoad};
use crate::{
    data::{Agent, DataPaths, RECENT_COMMAND_LIMIT, Session, delete},
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
}

struct HistoryGroup {
    session: Session,
    entries: Vec<HistoryEntry>,
}

/// reuse a filename inventory while checking each candidate against the filesystem
pub(crate) struct TranscriptIndex {
    projects: PathBuf,
    directories: Option<HashMap<String, Vec<PathBuf>>>,
}

impl TranscriptIndex {
    pub(crate) fn new(root: &Path) -> Self {
        let projects = root.join("projects");
        // retain project order and let the filesystem decide case-sensitive matches
        let directories = (|| {
            let mut directories: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for directory in children(&projects) {
                if !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
                    continue;
                }
                for entry in std::fs::read_dir(&directory).ok()? {
                    let filename = entry.ok()?.file_name();
                    let filename = filename.to_str()?;
                    // keep the original lookup when a listing cannot represent every name
                    if !filename.is_ascii() {
                        return None;
                    }
                    let filename = filename.to_ascii_lowercase();
                    if filename.ends_with(".jsonl") {
                        directories
                            .entry(filename)
                            .or_default()
                            .push(directory.clone());
                    }
                }
            }
            Some(directories)
        })();
        Self {
            projects,
            directories,
        }
    }

    pub(crate) fn find(&self, id: &str) -> Option<PathBuf> {
        match &self.directories {
            Some(directories) if id.is_ascii() && !id.contains('/') => {
                let filename = format!("{id}.jsonl").to_ascii_lowercase();
                let candidates = directories.get(&filename).into_iter().flatten().cloned();
                transcript_in_projects(id, &self.projects, candidates)
            }
            _ => transcript_in_projects(id, &self.projects, children(&self.projects).into_iter()),
        }
    }
}

pub(crate) fn transcript(id: &str, root: &Path) -> Option<PathBuf> {
    let projects = root.join("projects");
    transcript_in_projects(id, &projects, children(&projects).into_iter())
}

fn transcript_in_projects(
    id: &str,
    projects: &Path,
    directories: impl Iterator<Item = PathBuf>,
) -> Option<PathBuf> {
    if !matches!(
        Path::new(id).components().collect::<Vec<_>>().as_slice(),
        [std::path::Component::Normal(_)]
    ) {
        return None;
    }
    directories
        .filter(|path| std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()))
        .map(|directory| directory.join(format!("{id}.jsonl")))
        .find(|path| path.is_file() && is_within(path, projects))
}

fn read_sessions(paths: &DataPaths, load: SessionLoad<'_>) -> Result<Vec<Session>, String> {
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
    let mut groups: Vec<HistoryGroup> = Vec::new();
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
            || !load.matches_id(id)
        {
            continue;
        }
        let Some(date) = Utc.timestamp_millis_opt(timestamp).single() else {
            continue;
        };
        let index = *indexes.entry(id.to_owned()).or_insert_with(|| {
            groups.push(HistoryGroup {
                session: Session {
                    agent: Agent::Claude,
                    session_id: id.into(),
                    project_path: project.into(),
                    created_at: date,
                    last_active_at: date,
                    latest_command: String::new(),
                    commands: vec![],
                    name: None,
                    todos: vec![],
                },
                entries: Vec::new(),
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        if date < group.session.created_at {
            group.session.created_at = date;
            group.session.project_path = project.into();
        }
        group.session.last_active_at = group.session.last_active_at.max(date);
        let entry = HistoryEntry {
            timestamp,
            display: display.into(),
        };
        if load.is_full() {
            group.entries.push(entry);
        } else {
            // retain the last five entries in timestamp order, including stable ties
            let index = group
                .entries
                .partition_point(|entry| entry.timestamp <= timestamp);
            group.entries.insert(index, entry);
            if group.entries.len() > RECENT_COMMAND_LIMIT {
                group.entries.remove(0);
            }
        }
    }
    let mut sessions = Vec::new();
    for mut group in groups {
        group.entries.sort_by_key(|entry| entry.timestamp);
        group.session.latest_command = group.entries.last().unwrap().display.clone();
        group.session.commands = group
            .entries
            .into_iter()
            .map(|entry| entry.display)
            .collect();
        sessions.push(group.session);
    }
    if load.is_full() && !sessions.is_empty() {
        let indices: Vec<_> = (0..sessions.len()).collect();
        Claude.load_names(&mut sessions, &indices, paths);
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.last_active_at));
    Ok(sessions)
}

impl AgentProvider for Claude {
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

    fn load_names(&self, sessions: &mut [Session], indices: &[usize], paths: &DataPaths) {
        let transcripts = TranscriptIndex::new(&paths.claude);
        for &index in indices {
            let session = &mut sessions[index];
            let mut custom = None;
            let mut generated = None;
            if let Some(path) = transcripts.find(&session.session_id) {
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
            session.name = custom.or(generated);
        }
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
