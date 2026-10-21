//! local deletion of session artifacts, with shared-data and root checks

use super::{Session, agents::TranscriptIndex};
use crate::utils::{
    CommandRunner, children, is_within, project_files, remove_jsonl_lines, visit_jsonl,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
};

pub fn claude_sessions(
    sessions: &[&Session],
    root: &Path,
    runner: &dyn CommandRunner,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut deletable = Vec::new();
    let mut jobs = None;
    for session in sessions {
        let job = jobs
            .get_or_insert_with(|| {
                let mut jobs = HashMap::new();
                for path in project_files(&root.join("jobs"), "json") {
                    if !path.file_name().is_some_and(|name| name == "state.json") {
                        continue;
                    }
                    let Some(entry) = fs::read(&path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    else {
                        continue;
                    };
                    if let Some(id) = entry["sessionId"].as_str() {
                        jobs.entry(id.to_owned()).or_insert(path);
                    }
                }
                jobs
            })
            .get(&session.session_id)
            .cloned();
        if let Some(job) = job {
            let result = runner.run(&[
                "claude".into(),
                "rm".into(),
                job.parent().unwrap().file_name().unwrap().into(),
            ]);
            // agent commands may change other jobs even when they report failure
            jobs = None;
            if let Err(error) = result {
                errors.push(error);
                continue;
            }
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
    let transcripts = TranscriptIndex::new(root);
    let projects = root.join("projects");
    let mut plan_references: Option<PlanReferences> = None;
    let mut artifacts = HashMap::new();
    find_artifacts(root, &ids, &mut artifacts);
    for matches in artifacts.values_mut() {
        matches.sort();
    }
    for session in deletable {
        let path = transcripts.find(&session.session_id);
        let slug = path.as_ref().and_then(|path| {
            if let Some(references) = &plan_references
                && let Some(slug) = references.first_slugs.get(path)
            {
                return slug.clone();
            }
            read_transcript_plans(path).first_slug
        });
        if let Some(path) = &path {
            remove_artifact(path, root, &mut errors);
            remove_artifact(&path.with_extension(""), root, &mut errors);
        }
        for artifact in artifacts.get(&session.session_id).into_iter().flatten() {
            remove_artifact(artifact, root, &mut errors);
        }
        if let Some(slug) = slug {
            let references = plan_references.get_or_insert_with(|| PlanReferences::new(&projects));
            if !references.in_use(&slug, &projects) {
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

struct TranscriptPlans {
    first_slug: Option<String>,
    slugs: HashSet<String>,
    readable: bool,
}

fn read_transcript_plans(path: &Path) -> TranscriptPlans {
    let mut first_slug = None;
    let mut slugs = HashSet::new();
    let readable = visit_jsonl(path, |entry| {
        if let Some(slug) = entry["slug"].as_str().filter(|name| safe_name(name)) {
            if first_slug.is_none() {
                first_slug = Some(slug.to_owned());
            }
            slugs.insert(slug.to_owned());
        }
    })
    .is_ok();
    TranscriptPlans {
        first_slug,
        slugs,
        readable,
    }
}

struct PlanReferences {
    first_slugs: HashMap<PathBuf, Option<String>>,
    by_slug: HashMap<String, VecDeque<PathBuf>>,
    unreadable: VecDeque<PathBuf>,
}

impl PlanReferences {
    fn new(projects: &Path) -> Self {
        let mut references = Self {
            first_slugs: HashMap::new(),
            by_slug: HashMap::new(),
            unreadable: VecDeque::new(),
        };
        for path in project_files(projects, "jsonl") {
            let plans = read_transcript_plans(&path);
            references
                .first_slugs
                .insert(path.clone(), plans.first_slug);
            for slug in plans.slugs {
                references
                    .by_slug
                    .entry(slug)
                    .or_default()
                    .push_back(path.clone());
            }
            if !plans.readable {
                references.unreadable.push_back(path);
            }
        }
        references
    }

    fn in_use(&mut self, slug: &str, projects: &Path) -> bool {
        self.by_slug
            .get_mut(slug)
            .is_some_and(|paths| live_reference(paths, projects))
            || live_reference(&mut self.unreadable, projects)
    }
}

fn live_reference(paths: &mut VecDeque<PathBuf>, projects: &Path) -> bool {
    while let Some(path) = paths.front() {
        if path.is_file() && is_within(path, projects) {
            return true;
        }
        // discard removed references so later sessions do not check them again
        paths.pop_front();
    }
    false
}

fn find_artifacts(
    directory: &Path,
    ids: &HashSet<String>,
    matches: &mut HashMap<String, Vec<PathBuf>>,
) {
    for path in children(directory) {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if let Some(name) = path.file_name().filter(|name| *name != "history.jsonl") {
            let name = name.to_string_lossy();
            for id in ids {
                if name.contains(id) {
                    matches.entry(id.clone()).or_default().push(path.clone());
                }
            }
        }
        if metadata.is_dir() {
            find_artifacts(&path, ids, matches);
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
    match fs::File::open(history) {
        Ok(file) => {
            let mut reader = io::BufReader::new(file);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => return,
                }
                // safe hashes contain no newlines, so substring matches stay within a line
                let text = String::from_utf8_lossy(&line);
                hashes.retain(|hash| !text.contains(hash.as_str()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return,
    }
    for hash in hashes {
        remove_artifact(
            &root.join("paste-cache").join(format!("{hash}.txt")),
            root,
            errors,
        );
    }
}
