//! filesystem, formatting, and command helpers shared by the providers

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const SHORT_ID_LENGTH: usize = 8;

pub fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.into();
    }
    if limit == 0 {
        return String::new();
    }
    text.chars().take(limit - 1).chain(['…']).collect()
}

pub fn format_datetime(date: DateTime<Utc>) -> String {
    date.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

pub fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_rfc3339(text) {
        return Some(date.with_timezone(&Utc));
    }
    let date = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Local
        .from_local_datetime(&date)
        .earliest()
        .map(|date| date.with_timezone(&Utc))
}

pub fn open_readonly(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

/// keep every unmatched line byte-for-byte and replace the file only on success
pub fn remove_jsonl_lines(
    path: &Path,
    id_key: &str,
    ids: &HashSet<String>,
) -> io::Result<Vec<Value>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error),
    };
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let entry = serde_json::from_slice::<Value>(line).ok();
        let matches = entry
            .as_ref()
            .and_then(|value| value.get(id_key))
            .and_then(Value::as_str)
            .is_some_and(|id| ids.contains(id));
        if matches {
            removed.push(entry.unwrap());
        } else {
            kept.extend_from_slice(line);
        }
    }
    if removed.is_empty() {
        return Ok(removed);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    temporary
        .as_file()
        .set_permissions(fs::metadata(path)?.permissions())?;
    temporary.write_all(&kept)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(removed)
}

pub fn children(path: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

pub fn is_within(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// find files one project directory below a data root without following links
pub fn project_files(root: &Path, extension: &str) -> Vec<PathBuf> {
    children(root)
        .into_iter()
        .filter(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()))
        .flat_map(|directory| children(&directory))
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == extension)
                && path.is_file()
                && is_within(path, root)
        })
        .collect()
}

pub fn visit_jsonl(path: &Path, visit: impl FnMut(Value)) -> io::Result<()> {
    visit_jsonl_matching(path, &[], visit)
}

pub(crate) fn visit_jsonl_matching(
    path: &Path,
    markers: &[&str],
    mut visit: impl FnMut(Value),
) -> io::Result<()> {
    let file = fs::File::open(path)?;
    for line in io::BufReader::new(file).split(b'\n') {
        let line = line?;
        let text = String::from_utf8_lossy(&line);
        if !markers.is_empty() && !markers.iter().any(|marker| text.contains(marker)) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value.is_object() {
            visit(value);
        }
    }
    Ok(())
}

pub trait CommandRunner {
    fn run(&self, argv: &[OsString]) -> Result<(), String>;
}

pub struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&self, argv: &[OsString]) -> Result<(), String> {
        let Some(program) = argv.first() else {
            return Err("Empty agent command".into());
        };
        let command = argv
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let output = Command::new(program)
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("{command}: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let bytes = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let text = String::from_utf8_lossy(bytes);
        let reason = text
            .trim()
            .lines()
            .last()
            .map(String::from)
            .unwrap_or_else(|| match output.status.code() {
                Some(code) => format!("exit code {code}"),
                None => "terminated by signal".into(),
            });
        Err(format!("{command}: {reason}"))
    }
}
