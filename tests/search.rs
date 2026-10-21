mod common;

use agents_sessions::{
    data::Agent,
    tui::{Action, DirectoryScope, Mode, Palette, State, render},
};
use chrono::{Local, TimeZone, Utc};
use common::session;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use serde_json::json;
use std::collections::HashSet;

fn key(state: &mut State, code: KeyCode) -> Action {
    state.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn toggle(state: &mut State) -> Action {
    state.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
}

fn type_query(state: &mut State, text: &str) {
    for character in text.chars() {
        assert_eq!(key(state, KeyCode::Char(character)), Action::None);
    }
}

fn screen(state: &mut State, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render(frame, state, &Palette::new(16_777_216)))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn search_matches_only_full_names_paths_ids_and_modified_text() {
    let mut named = session(Agent::Pi, "name-id");
    named.name = Some("A long chat name with a hidden suffix: CAFÉ".into());
    let mut path = session(Agent::Pi, "path-id");
    path.project_path = "/parent-directory/project".into();
    let id = session(Agent::Pi, "01234567-hidden-id-suffix");
    let mut dated = session(Agent::OpenCode, "date-id");
    dated.created_at = Utc.with_ymd_and_hms(2048, 7, 1, 12, 0, 0).unwrap();
    dated.last_active_at = Local
        .with_ymd_and_hms(2026, 10, 24, 14, 30, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let mut state = State::new(vec![named, path, id, dated], "/p/proj".into());
    key(&mut state, KeyCode::Char('f'));
    for (query, expected) in [
        ("café", vec![0]),
        ("PARENT-DIRECTORY", vec![1]),
        ("HIDDEN-ID-SUFFIX", vec![2]),
        ("2026-10-24 14:30", vec![3]),
        ("10-2", vec![3]),
        ("10", vec![3]),
        ("hello", vec![]),
        ("opencode", vec![]),
        ("2048", vec![]),
    ] {
        toggle(&mut state);
        type_query(&mut state, query);
        assert_eq!(state.visible, expected, "query: {query}");
        toggle(&mut state);
        assert_eq!(state.visible, vec![0, 1, 2, 3]);
    }
}

#[test]
fn calendar_queries_use_local_modified_dates_and_keep_literal_name_matches() {
    let today = Local::now();
    let mut current = session(Agent::Pi, "current");
    current.last_active_at = today.with_timezone(&Utc);
    let mut previous = session(Agent::Pi, "previous");
    previous.last_active_at = today
        .checked_sub_days(chrono::Days::new(1))
        .unwrap()
        .with_timezone(&Utc);
    let mut named = session(Agent::Pi, "named");
    named.name = Some("Today notes".into());
    let mut state = State::new(vec![current, previous, named], "/p/proj".into());
    toggle(&mut state);
    type_query(&mut state, "TODAY");
    assert_eq!(state.visible, vec![0, 2]);
    toggle(&mut state);
    toggle(&mut state);
    type_query(&mut state, "yesterday");
    assert_eq!(state.visible, vec![1]);
}

#[test]
fn backspace_removes_unicode_characters_without_damaging_the_query() {
    let mut item = session(Agent::Pi, "id");
    item.name = Some("café".into());
    let mut state = State::new(vec![item], "/p/proj".into());
    toggle(&mut state);
    type_query(&mut state, "café界");
    assert!(state.visible.is_empty());
    key(&mut state, KeyCode::Backspace);
    assert_eq!(state.visible, vec![0]);
    assert!(screen(&mut state, 180, 10).contains("Search: café"));
    key(&mut state, KeyCode::Backspace);
    assert!(screen(&mut state, 180, 10).contains("Search: caf  |"));
}

#[test]
fn search_keeps_agent_directory_filters_and_activity_order_in_both_modes() {
    let mut elsewhere = session(Agent::Codex, "match-other");
    elsewhere.project_path = "/elsewhere".into();
    for mode in [Mode::Select, Mode::Delete] {
        let mut state = State::new(
            vec![
                session(Agent::Claude, "match-claude"),
                session(Agent::Codex, "match-first"),
                elsewhere.clone(),
                session(Agent::Codex, "match-last"),
            ],
            "/p/proj".into(),
        );
        state.mode = mode;
        key(&mut state, KeyCode::Char('2'));
        toggle(&mut state);
        type_query(&mut state, "match");
        assert_eq!(state.visible, vec![1, 3]);
        toggle(&mut state);
        assert_eq!(state.visible, vec![1, 3]);
        key(&mut state, KeyCode::Char('f'));
        toggle(&mut state);
        type_query(&mut state, "match");
        assert_eq!(state.visible, vec![1, 2, 3]);
        assert_eq!(state.filters, HashSet::from([Agent::Codex]));
        assert_eq!(state.directory_scope, DirectoryScope::All);
        assert_eq!(state.mode, mode);
    }
}

#[test]
fn shortcut_characters_are_text_during_search() {
    let text = "fFdDqQaA1234 jk-/";
    for mode in [Mode::Select, Mode::Delete] {
        let mut item = session(Agent::Pi, "id");
        item.name = Some(text.into());
        let mut state = State::new(vec![item], "/p/proj".into());
        state.mode = mode;
        toggle(&mut state);
        type_query(&mut state, text);
        assert_eq!(state.visible, vec![0]);
        assert_eq!(state.mode, mode);
        assert_eq!(state.directory_scope, DirectoryScope::Cwd);
        assert_eq!(state.filters.len(), 4);
        assert!(state.selected.is_empty());
        assert!(screen(&mut state, 180, 10).contains(text));
    }
}

#[test]
fn query_edits_reset_navigation_and_backspace_can_restore_empty_results() {
    let mut state = State::new(
        vec![session(Agent::Pi, "c1"), session(Agent::Pi, "c2")],
        "/p/proj".into(),
    );
    toggle(&mut state);
    type_query(&mut state, "c");
    key(&mut state, KeyCode::Down);
    assert_eq!(state.cursor, 1);
    state.scroll_offset = 1;
    type_query(&mut state, "2");
    assert_eq!(state.visible, vec![1]);
    assert_eq!((state.cursor, state.scroll_offset), (0, 0));
    type_query(&mut state, "z");
    assert!(state.visible.is_empty());
    assert_eq!(key(&mut state, KeyCode::Enter), Action::None);
    key(&mut state, KeyCode::Backspace);
    assert_eq!(state.visible, vec![1]);
    key(&mut state, KeyCode::Backspace);
    key(&mut state, KeyCode::Backspace);
    key(&mut state, KeyCode::Backspace);
    assert_eq!(state.visible, vec![0, 1]);
    type_query(&mut state, "nothing");
    toggle(&mut state);
    assert_eq!(state.visible, vec![0, 1]);
    toggle(&mut state);
    assert_eq!(state.visible, vec![0, 1]);
}

#[test]
fn arrows_and_enter_still_navigate_and_launch_in_search() {
    let mut state = State::new(
        vec![session(Agent::Pi, "match-1"), session(Agent::Pi, "match-2")],
        "/p/proj".into(),
    );
    toggle(&mut state);
    type_query(&mut state, "match");
    key(&mut state, KeyCode::Down);
    assert_eq!(state.current().unwrap().session_id, "match-2");
    assert_eq!(key(&mut state, KeyCode::Enter), Action::Launch);
    key(&mut state, KeyCode::Up);
    assert_eq!(state.current().unwrap().session_id, "match-1");
}

#[test]
fn search_preserves_quit_keys_and_ignores_modified_or_released_text() {
    for mode in [Mode::Select, Mode::Delete] {
        let mut state = State::new(vec![], "/p/proj".into());
        state.mode = mode;
        toggle(&mut state);
        assert_eq!(key(&mut state, KeyCode::Esc), Action::Quit);
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
        for event in [
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            KeyEvent::new_with_kind(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ),
            KeyEvent::new_with_kind(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL,
                KeyEventKind::Release,
            ),
        ] {
            assert_eq!(state.handle_key(event), Action::None);
        }
        assert!(screen(&mut state, 180, 10).contains("Search:   |"));
    }
}

#[test]
fn search_unselects_hidden_rows_and_preserves_tab_ranges_and_confirmation() {
    let mut state = State::new(
        vec![
            session(Agent::Pi, "match-1"),
            session(Agent::Pi, "hidden"),
            session(Agent::Pi, "match-2"),
        ],
        "/p/proj".into(),
    );
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Char('a'));
    toggle(&mut state);
    type_query(&mut state, "match");
    assert_eq!(state.selected.len(), 2);
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::BackTab);
    assert_eq!(state.selected.len(), 2);
    key(&mut state, KeyCode::Enter);
    assert!(state.confirming);
    assert!(screen(&mut state, 180, 10).contains("Delete 2 session(s)?"));
    toggle(&mut state);
    assert!(!state.confirming);
    assert_eq!(state.visible, vec![0, 2]);
    assert!(screen(&mut state, 180, 10).contains("Search: match"));
    key(&mut state, KeyCode::Enter);
    assert_eq!(key(&mut state, KeyCode::Char('y')), Action::Delete);
    toggle(&mut state);
    assert_eq!(state.visible, vec![0, 1, 2]);
    assert_eq!(state.selected.len(), 2);
}

#[test]
fn search_loads_offscreen_names_even_with_no_provisional_matches_and_reuses_them() {
    let root = tempfile::tempdir().unwrap();
    let paths = common::paths(root.path());
    let mut sessions = Vec::new();
    for index in 0..12 {
        let id = format!("id-{index}");
        common::write_jsonl(
            &paths.claude.join(format!("projects/project/{id}.jsonl")),
            &[json!({"type":"custom-title","customTitle":format!("name-{index}")})],
        );
        let mut item = session(Agent::Claude, &id);
        if index == 11 {
            item.project_path = "/elsewhere".into();
        }
        sessions.push(item);
    }
    let mut state = State::new(sessions, "/p/proj".into());
    let area = Rect::new(0, 0, 100, 8);
    state.prepare_frame(&paths, area);
    assert!(state.all_sessions[9].name.is_none());
    toggle(&mut state);
    type_query(&mut state, "name-9");
    assert!(state.visible.is_empty());
    state.prepare_frame(&paths, area);
    assert_eq!(state.visible, vec![9]);
    assert!(
        state.all_sessions[..11]
            .iter()
            .all(|item| item.name.is_some())
    );
    assert!(state.all_sessions[11].name.is_none());
    common::write_jsonl(
        &paths.claude.join("projects/project/id-9.jsonl"),
        &[json!({"type":"custom-title","customTitle":"changed"})],
    );
    key(&mut state, KeyCode::Backspace);
    type_query(&mut state, "9");
    state.prepare_frame(&paths, area);
    assert_eq!(state.current().unwrap().name.as_deref(), Some("name-9"));
}

#[test]
fn query_edits_reuse_directory_candidates_without_rechecking_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let other = root.path().join("other");
    let alias = root.path().join("alias");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&other).unwrap();
    std::os::unix::fs::symlink(&project, &alias).unwrap();
    let mut item = session(Agent::Pi, "match");
    item.project_path = alias.clone();
    let mut state = State::new(vec![item], project);
    toggle(&mut state);
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(other, alias).unwrap();
    type_query(&mut state, "match");
    assert_eq!(state.visible, vec![0]);
    toggle(&mut state);
    assert_eq!(state.visible, vec![0]);
    key(&mut state, KeyCode::Char('f'));
    key(&mut state, KeyCode::Char('f'));
    assert!(state.visible.is_empty());
}

#[test]
fn deletion_rebuilds_search_candidates_and_preserves_the_query() {
    use agents_sessions::{data::agents::load_session_summaries, utils::CommandRunner};
    use std::ffi::OsString;
    struct NoCommands;
    impl CommandRunner for NoCommands {
        fn run(&self, _: &[OsString]) -> Result<(), String> {
            panic!("Claude deletion must not launch a command");
        }
    }
    let root = tempfile::tempdir().unwrap();
    let paths = common::paths(root.path());
    common::write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[
            json!({"sessionId":"first","timestamp":3000,"project":"/p/proj","display":"hi"}),
            json!({"sessionId":"second","timestamp":2000,"project":"/p/proj","display":"hi"}),
            json!({"sessionId":"third","timestamp":1000,"project":"/p/proj","display":"hi"}),
        ],
    );
    for (id, name) in [("first", "keep"), ("second", "keep"), ("third", "other")] {
        common::write_jsonl(
            &paths.claude.join(format!("projects/project/{id}.jsonl")),
            &[json!({"type":"custom-title","customTitle":name})],
        );
    }
    let mut state = State::new(
        load_session_summaries(&paths, None).unwrap(),
        "/p/proj".into(),
    );
    key(&mut state, KeyCode::Char('d'));
    toggle(&mut state);
    let area = Rect::new(0, 0, 180, 30);
    state.prepare_frame(&paths, area);
    type_query(&mut state, "keep");
    key(&mut state, KeyCode::Tab);
    state.delete_selected(&paths, &NoCommands).unwrap();
    state.prepare_frame(&paths, area);
    assert_eq!(state.mode, Mode::Select);
    assert_eq!(state.visible.len(), 1);
    assert_eq!(state.current().unwrap().session_id, "second");
    assert!(screen(&mut state, 180, 30).contains("Deleted 1 session(s)."));
    key(&mut state, KeyCode::Backspace);
    assert!(screen(&mut state, 180, 30).contains("Search: kee"));
    toggle(&mut state);
    assert_eq!(state.visible.len(), 2);
}

#[test]
fn search_footer_survives_empty_and_small_screens_and_yields_to_messages() {
    let mut state = State::new(vec![], "/p/proj".into());
    assert!(screen(&mut state, 180, 10).contains("Ctrl+F: Search"));
    toggle(&mut state);
    type_query(&mut state, "no match");
    let rendered = screen(&mut state, 180, 10);
    assert!(rendered.contains("No sessions found."));
    assert!(rendered.contains("Search: no match"));
    assert!(rendered.contains("Ctrl+F: Clear"));
    assert!(!rendered.contains("ESC/Q"));
    assert!(!rendered.contains("F: CWD/All"));
    type_query(&mut state, &"界é".repeat(200));
    type_query(&mut state, "query-end");
    assert!(screen(&mut state, 20, 4).contains("query-end"));
    for (width, height) in [(0, 0), (1, 1), (19, 3), (20, 4), (50, 8), (60, 10)] {
        screen(&mut state, width, height);
    }
    state.message = Some("Error: test failure".into());
    assert!(screen(&mut state, 180, 10).contains("Error: test failure"));
}
