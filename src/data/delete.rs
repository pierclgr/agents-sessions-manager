//! local deletion of session artifacts, with shared-data and root checks

use super::{Session, agents::transcript};
use crate::utils::{
    CommandRunner, children, is_within, project_files, remove_jsonl_lines, visit_jsonl,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

pub fn claude_sessions(
    sessions: &[&Session],
    root: &Path,
    runner: &dyn CommandRunner,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut deletable = Vec::new();
    for session in sessions {
        let job = project_files(&root.join("jobs"), "json")
            .into_iter()
            .find(|path| {
                path.file_name().is_some_and(|name| name == "state.json")
                    && fs::read(path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                        .is_some_and(|value| {
                            value["sessionId"].as_str() == Some(&session.session_id)
                        })
            });
        if let Some(job) = job
            && let Err(error) = runner.run(&[
                "claude".into(),
                "rm".into(),
                job.parent().unwrap().file_name().unwrap().into(),
            ])
        {
            errors.push(error);
            continue;
        }
        deletable.push(*session);
    }
    if deletable.is_empty() {
        return errors;
    }
    let ids = deletable
        .iter()
        .map(|session| session.session_id.clone())
        .collect();
    let history = root.join("history.jsonl");
    if history.exists() && !is_within(&history, root) {
        errors.push("Could not rewrite history.jsonl: path is outside the data directory".into());
        return errors;
    }
    let removed = match remove_jsonl_lines(&history, "sessionId", &ids) {
        Ok(removed) => removed,
        Err(error) => {
            errors.push(format!("Could not rewrite history.jsonl: {error}"));
            return errors;
        }
    };
    delete_unused_pastes(&removed, &history, root, &mut errors);
    for session in deletable {
        let path = transcript(&session.session_id, root);
        let mut slug = None;
        if let Some(path) = &path {
            let _ = visit_jsonl(path, |entry| {
                if slug.is_none() {
                    slug = entry["slug"]
                        .as_str()
                        .filter(|name| safe_name(name))
                        .map(String::from);
                }
            });
            remove_artifact(path, root, &mut errors);
            remove_artifact(&path.with_extension(""), root, &mut errors);
        }
        let mut artifacts = Vec::new();
        find_artifacts(root, &session.session_id, &mut artifacts);
        artifacts.sort();
        for artifact in artifacts {
            remove_artifact(&artifact, root, &mut errors);
        }
        if let Some(slug) = slug {
            // retain a shared plan when another transcript cannot be checked
            let in_use = project_files(&root.join("projects"), "jsonl")
                .iter()
                .any(|path| {
                    let mut found = false;
                    visit_jsonl(path, |entry| {
                        found |= entry["slug"].as_str() == Some(&slug);
                    })
                    .is_err()
                        || found
                });
            if !in_use {
                remove_artifact(
                    &root.join("plans").join(format!("{slug}.md")),
                    root,
                    &mut errors,
                );
            }
        }
        if let Some(path) = path {
            let project = path.parent().unwrap();
            if is_within(project, root) {
                let _ = fs::remove_dir(project);
            }
        }
    }
    errors
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn find_artifacts(directory: &Path, id: &str, matches: &mut Vec<PathBuf>) {
    for path in children(directory) {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if path
            .file_name()
            .is_some_and(|name| name != "history.jsonl" && name.to_string_lossy().contains(id))
        {
            matches.push(path.clone());
        }
        if metadata.is_dir() {
            find_artifacts(&path, id, matches);
        }
    }
}

fn remove_artifact(path: &Path, root: &Path, errors: &mut Vec<String>) {
    if !is_within(path, root) || path.canonicalize().ok() == root.canonicalize().ok() {
        return;
    }
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        errors.push(format!("Could not delete {}: {error}", path.display()));
    }
}

fn delete_unused_pastes(removed: &[Value], history: &Path, root: &Path, errors: &mut Vec<String>) {
    let mut hashes = HashSet::new();
    for entry in removed {
        if let Some(pastes) = entry["pastedContents"].as_object() {
            for item in pastes.values() {
                if let Some(hash) = item["contentHash"].as_str().filter(|name| safe_name(name)) {
                    hashes.insert(hash.to_owned());
                }
            }
        }
    }
    if hashes.is_empty() {
        return;
    }
    let bytes = match fs::read(history) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return,
    };
    let remaining = String::from_utf8_lossy(&bytes);
    for hash in hashes {
        if !remaining.contains(&hash) {
            remove_artifact(
                &root.join("paste-cache").join(format!("{hash}.txt")),
                root,
                errors,
            );
        }
    }
}
