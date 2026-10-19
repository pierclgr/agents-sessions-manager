mod common;

use agents_sessions::{
    data::DataPaths,
    utils::{remove_jsonl_lines, truncate},
};
use serde_json::json;
use std::{collections::HashSet, ffi::OsString, fs};

#[test]
fn environment_defaults_and_override_precedence() {
    let home = std::path::PathBuf::from("/home/test");
    let defaults = DataPaths::from_lookup(home.clone(), |_| None);
    assert_eq!(defaults.claude, home.join(".claude"));
    assert_eq!(defaults.codex, home.join(".codex"));
    assert_eq!(
        defaults.opencode_db,
        home.join(".local/share/opencode/opencode.db")
    );
    assert_eq!(defaults.pi_sessions, home.join(".pi/agent/sessions"));
    let values = [
        ("CLAUDE_DATA_DIR", "/custom/claude"),
        ("CODEX_HOME", "/custom/codex"),
        ("XDG_DATA_HOME", "/custom/data"),
        ("PI_CODING_AGENT_DIR", "/custom/pi"),
        ("PI_CODING_AGENT_SESSION_DIR", "/custom/sessions"),
    ];
    let paths = DataPaths::from_lookup(home.clone(), |key| {
        values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| OsString::from(value))
    });
    assert_eq!(paths.claude, std::path::Path::new("/custom/claude"));
    assert_eq!(paths.codex, std::path::Path::new("/custom/codex"));
    assert_eq!(
        paths.opencode_db,
        std::path::Path::new("/custom/data/opencode/opencode.db")
    );
    assert_eq!(paths.pi_sessions, std::path::Path::new("/custom/sessions"));
    let paths = DataPaths::from_lookup(home, |key| {
        (key == "PI_CODING_AGENT_DIR").then(|| OsString::from("/custom/pi"))
    });
    assert_eq!(
        paths.pi_sessions,
        std::path::Path::new("/custom/pi/sessions")
    );
}

#[test]
fn empty_environment_values_use_defaults() {
    let paths = DataPaths::from_lookup("/home/test".into(), |_| Some(OsString::new()));
    assert_eq!(paths.claude, std::path::Path::new("/home/test/.claude"));
}

#[test]
fn unicode_truncation_handles_small_limits() {
    assert_eq!(truncate("é日🙂test", 4), "é日🙂…");
    assert_eq!(truncate("hello", 1), "…");
    assert_eq!(truncate("hello", 0), "");
    assert_eq!(truncate("é日", 2), "é日");
}

#[test]
fn history_rewrite_preserves_unrelated_bytes_and_malformed_lines() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("history.jsonl");
    let original = b"\nnot json\n{\"id\":\"keep\"}\r\n{\"id\":\"delete\",\"x\":1}\n[]\n\xff";
    fs::write(&file, original).unwrap();
    let removed = remove_jsonl_lines(&file, "id", &HashSet::from(["delete".into()])).unwrap();
    assert_eq!(removed, vec![json!({"id":"delete", "x":1})]);
    assert_eq!(
        fs::read(&file).unwrap(),
        b"\nnot json\n{\"id\":\"keep\"}\r\n[]\n\xff"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn missing_or_unmatched_history_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("history.jsonl");
    let ids = HashSet::from(["missing".into()]);
    assert!(remove_jsonl_lines(&file, "id", &ids).unwrap().is_empty());
    fs::write(&file, "unchanged").unwrap();
    assert!(remove_jsonl_lines(&file, "id", &ids).unwrap().is_empty());
    assert_eq!(fs::read_to_string(file).unwrap(), "unchanged");
}

#[test]
fn failed_atomic_rewrite_keeps_the_original_and_leaves_no_temporary_file() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("history.jsonl");
    let original = b"{\"id\":\"delete\"}\n{\"id\":\"keep\"}\n";
    fs::write(&file, original).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = remove_jsonl_lines(&file, "id", &HashSet::from(["delete".into()]));
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
