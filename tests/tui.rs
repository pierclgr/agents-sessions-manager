mod common;

use agents_sessions::{
    data::Agent,
    tui::{Action, DirectoryScope, Mode, Palette, State, render},
};
use common::session;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Color};
use std::collections::HashSet;

fn state() -> State {
    State::new(
        vec![
            session(Agent::Claude, "c1"),
            session(Agent::Codex, "x1"),
            session(Agent::Claude, "c2"),
            session(Agent::Pi, "p1"),
        ],
        "/p/proj".into(),
    )
}

fn key(state: &mut State, code: KeyCode) -> Action {
    state.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn all_agents_start_visible_and_number_keys_toggle_a_single_agent() {
    let mut state = state();
    assert_eq!(state.filters.len(), 4);
    key(&mut state, KeyCode::Char('1'));
    assert_eq!(state.filters, HashSet::from([Agent::ALL[0]]));
    assert_eq!(state.visible.len(), 2);
    key(&mut state, KeyCode::Char('2'));
    assert_eq!(state.filters, HashSet::from([Agent::ALL[1]]));
    key(&mut state, KeyCode::Char('2'));
    assert_eq!(state.filters.len(), 4);
    assert_eq!(state.visible.len(), 4);
}

#[test]
fn startup_scope_matches_only_the_exact_launch_directory() {
    let sessions = ["/p/proj", "/p/proj/child", "/p/proj-other", "/p"]
        .into_iter()
        .enumerate()
        .map(|(index, project)| {
            let mut session = session(Agent::Claude, &format!("c{index}"));
            session.project_path = project.into();
            session
        })
        .collect();
    let mut state = State::new(sessions, "/p/proj".into());
    assert_eq!(state.directory_scope, DirectoryScope::Cwd);
    assert_eq!(state.visible, vec![0]);
    assert_eq!(state.all_sessions.len(), 4);
    key(&mut state, KeyCode::Char('f'));
    assert_eq!(state.directory_scope, DirectoryScope::All);
    assert_eq!(state.visible, vec![0, 1, 2, 3]);
    assert_eq!(state.mode, Mode::Select);
    key(&mut state, KeyCode::Char('F'));
    assert_eq!(state.directory_scope, DirectoryScope::Cwd);
    assert_eq!(state.visible, vec![0]);
}

#[test]
fn cwd_scope_matches_symlink_aliases_in_either_path() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let alias = root.path().join("alias");
    std::fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&project, &alias).unwrap();
    for (cwd, path) in [(project.clone(), alias.clone()), (alias, project)] {
        let mut session = session(Agent::Claude, "c1");
        session.project_path = path.clone();
        let mut state = State::new(vec![session], cwd);
        assert_eq!(state.visible, vec![0]);
        assert_eq!(state.current().unwrap().project_path, path);
        key(&mut state, KeyCode::Char('f'));
        key(&mut state, KeyCode::Char('f'));
        assert_eq!(state.visible, vec![0]);
    }
}

#[test]
fn directory_filter_refreshes_alias_matches_between_operations() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let other = root.path().join("other");
    let alias = root.path().join("alias");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&other).unwrap();
    let sessions = ["first", "second"]
        .into_iter()
        .map(|id| {
            let mut item = session(Agent::Claude, id);
            item.project_path = alias.clone();
            item
        })
        .collect();
    let mut state = State::new(sessions, project.clone());
    assert!(state.visible.is_empty());
    for target in [&project, &other, &project] {
        key(&mut state, KeyCode::Char('f'));
        if alias.exists() {
            std::fs::remove_file(&alias).unwrap();
        }
        std::os::unix::fs::symlink(target, &alias).unwrap();
        key(&mut state, KeyCode::Char('f'));
        assert_eq!(state.visible.len(), if target == &project { 2 } else { 0 });
    }
}

#[test]
fn directory_and_agent_filters_combine_in_both_modes() {
    for mode in [Mode::Select, Mode::Delete] {
        let mut elsewhere = session(Agent::Codex, "other");
        elsewhere.project_path = "/elsewhere".into();
        let mut state = State::new(
            vec![
                session(Agent::Claude, "c1"),
                session(Agent::Codex, "x1"),
                elsewhere,
            ],
            "/p/proj".into(),
        );
        state.mode = mode;
        key(&mut state, KeyCode::Char('2'));
        assert_eq!(state.visible, vec![1]);
        key(&mut state, KeyCode::Char('f'));
        assert_eq!(state.visible, vec![1, 2]);
        assert_eq!(state.filters, HashSet::from([Agent::Codex]));
        key(&mut state, KeyCode::Char('F'));
        assert_eq!(state.visible, vec![1]);
        key(&mut state, KeyCode::Char('2'));
        assert_eq!(state.visible, vec![0, 1]);
        assert_eq!(state.mode, mode);
    }
}

#[test]
fn both_toggles_work_with_empty_results_and_empty_data() {
    for sessions in [vec![], vec![session(Agent::Claude, "c1")]] {
        let mut state = State::new(sessions, "/elsewhere".into());
        assert!(state.visible.is_empty());
        key(&mut state, KeyCode::Char('d'));
        assert_eq!(state.mode, Mode::Delete);
        key(&mut state, KeyCode::Char('f'));
        assert_eq!(state.directory_scope, DirectoryScope::All);
        assert_eq!(state.visible.len(), state.all_sessions.len());
        key(&mut state, KeyCode::Char('D'));
        assert_eq!(state.mode, Mode::Select);
        key(&mut state, KeyCode::Char('F'));
        assert_eq!(state.directory_scope, DirectoryScope::Cwd);
        assert!(state.visible.is_empty());
    }
}

#[test]
fn directory_filter_clamps_cursor_resets_scroll_and_drops_hidden_selections() {
    let mut state = state();
    state.all_sessions[3].project_path = "/elsewhere".into();
    key(&mut state, KeyCode::Char('f'));
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Char('a'));
    state.cursor = 3;
    state.scroll_offset = 2;
    key(&mut state, KeyCode::Char('F'));
    assert_eq!(state.cursor, 2);
    assert_eq!(state.scroll_offset, 0);
    assert_eq!(state.selected.len(), 3);
    assert!(!state.selected.contains(&(Agent::Pi, "p1".into())));
    key(&mut state, KeyCode::Char('f'));
    assert_eq!(state.visible.len(), 4);
    assert_eq!(state.selected.len(), 3);
}

#[test]
fn filtering_clamps_cursor_and_drops_hidden_selections() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Char('a'));
    state.cursor = 3;
    key(&mut state, KeyCode::Char('1'));
    assert_eq!(state.cursor, 1);
    assert_eq!(state.selected.len(), 2);
    key(&mut state, KeyCode::Char('2'));
    assert!(state.selected.is_empty());
}

#[test]
fn select_all_toggles_only_visible_sessions() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Char('1'));
    key(&mut state, KeyCode::Char('A'));
    assert_eq!(state.selected.len(), 2);
    key(&mut state, KeyCode::Char('a'));
    assert!(state.selected.is_empty());
}

#[test]
fn deselected_anchor_is_ignored_by_range_select() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::BackTab);
    assert_eq!(state.selected.len(), 1);
}

#[test]
fn tab_and_space_toggle_and_set_range_anchor() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Tab);
    assert_eq!(state.selected.len(), 1);
    key(&mut state, KeyCode::Char(' '));
    assert!(state.selected.is_empty());
    assert!(state.range_anchor.is_some());
}

#[test]
fn shift_tab_selects_ranges_in_both_directions() {
    for (start, end) in [(0, 2), (3, 1)] {
        let mut state = state();
        key(&mut state, KeyCode::Char('d'));
        state.cursor = start;
        key(&mut state, KeyCode::Tab);
        state.cursor = end;
        key(&mut state, KeyCode::BackTab);
        assert_eq!(state.selected.len(), 3);
    }
}

#[test]
fn missing_or_hidden_range_anchor_selects_only_current() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    state.cursor = 1;
    key(&mut state, KeyCode::BackTab);
    assert_eq!(state.selected.len(), 1);
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Char('2'));
    key(&mut state, KeyCode::BackTab);
    assert_eq!(state.selected.len(), 1);
}

#[test]
fn leaving_delete_mode_clears_selection_and_anchor() {
    let mut state = state();
    key(&mut state, KeyCode::Char('d'));
    key(&mut state, KeyCode::Tab);
    key(&mut state, KeyCode::Char('D'));
    assert_eq!(state.mode, Mode::Select);
    assert!(state.selected.is_empty());
    assert!(state.range_anchor.is_none());
    key(&mut state, KeyCode::Char('d'));
    state.cursor = 2;
    key(&mut state, KeyCode::BackTab);
    assert_eq!(state.selected.len(), 1);
}

#[test]
fn navigation_stays_in_bounds() {
    let mut state = state();
    key(&mut state, KeyCode::Up);
    assert_eq!(state.cursor, 0);
    for _ in 0..10 {
        key(&mut state, KeyCode::Char('j'));
    }
    assert_eq!(state.cursor, 3);
    key(&mut state, KeyCode::Char('k'));
    assert_eq!(state.cursor, 2);
}

#[test]
fn quit_keys_work_in_both_modes_and_empty_lists() {
    for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('Q')] {
        for empty in [false, true] {
            for delete in [false, true] {
                let mut state = if empty {
                    State::new(vec![], "/p/proj".into())
                } else {
                    state()
                };
                if delete {
                    state.mode = Mode::Delete;
                }
                assert_eq!(key(&mut state, code), Action::Quit);
            }
        }
    }
}

#[test]
fn confirmation_quit_keys_only_cancel_and_yes_requests_deletion() {
    for code in [
        KeyCode::Esc,
        KeyCode::Char('q'),
        KeyCode::Char('Q'),
        KeyCode::Char('n'),
        KeyCode::Char('f'),
        KeyCode::Char('F'),
        KeyCode::Char('d'),
        KeyCode::Char('D'),
    ] {
        let mut state = state();
        key(&mut state, KeyCode::Char('d'));
        key(&mut state, KeyCode::Tab);
        key(&mut state, KeyCode::Enter);
        assert!(state.confirming);
        assert_eq!(key(&mut state, code), Action::None);
        assert!(!state.confirming);
        assert_eq!(state.selected.len(), 1);
        assert_eq!(state.mode, Mode::Delete);
        assert_eq!(state.directory_scope, DirectoryScope::Cwd);
        key(&mut state, KeyCode::Enter);
        assert_eq!(key(&mut state, KeyCode::Char('Y')), Action::Delete);
    }
}

#[test]
fn select_enter_launches_and_delete_enter_requires_a_selection() {
    let mut state = state();
    assert_eq!(key(&mut state, KeyCode::Enter), Action::Launch);
    key(&mut state, KeyCode::Char('d'));
    assert_eq!(key(&mut state, KeyCode::Enter), Action::None);
    assert_eq!(state.message.as_deref(), Some("No sessions selected."));
}

fn screen(state: &mut State, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render(frame, state, &Palette::new(16_777_216)))
        .unwrap();
    terminal
}

fn text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn panes_show_names_short_ids_and_details() {
    let mut state = state();
    state.all_sessions[0].name = Some("data-reader".into());
    let terminal = screen(&mut state, 180, 40);
    let text = text(&terminal);
    for expected in [
        "Agent",
        "Project",
        "Session",
        "Modified",
        "Last message",
        "data-reader",
        "Session ID:",
        "Latest commands:",
        "Todos:",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert!(!text.contains("[ ] 1 Claude"));
}

#[test]
fn agent_names_have_colors_except_on_the_highlighted_row() {
    let mut state = state();
    let terminal = screen(&mut state, 180, 30);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 7)].bg, Color::Rgb(202, 124, 94));
    assert_eq!(buffer[(0, 8)].fg, Color::Rgb(135, 206, 250));
    assert_eq!(buffer[(8, 8)].fg, Color::Reset);
}

#[test]
fn selected_markers_are_keyed_by_agent_and_id() {
    let mut state = State::new(
        vec![
            session(Agent::Claude, "same"),
            session(Agent::Codex, "same"),
        ],
        "/p/proj".into(),
    );
    state.mode = Mode::Delete;
    state.selected.insert((Agent::Codex, "same".into()));
    let terminal = screen(&mut state, 180, 30);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 7)].symbol(), " ");
    assert_eq!(buffer[(1, 8)].symbol(), "x");
}

#[test]
fn tiny_short_narrow_and_empty_screens_render_without_panics() {
    for (width, height) in [(0, 0), (1, 1), (19, 3), (20, 4), (50, 8), (60, 10)] {
        for empty in [false, true] {
            let mut state = if empty {
                State::new(vec![], "/p/proj".into())
            } else {
                state()
            };
            let terminal = screen(&mut state, width, height);
            if width == 50 {
                assert!(!text(&terminal).contains("Session ID:"));
            }
        }
    }
}

#[test]
fn fallback_palette_preserves_agent_and_mode_distinction() {
    let palette = Palette::new(16);
    assert_eq!(palette.agent(Agent::Claude).fg, Some(Color::Red));
    assert_eq!(palette.agent(Agent::Codex).fg, Some(Color::Cyan));
    assert_eq!(palette.agent(Agent::OpenCode).fg, Some(Color::Green));
    assert_eq!(palette.agent(Agent::Pi).fg, Some(Color::Magenta));
    assert_eq!(palette.accent(Mode::Select).bg, Some(Color::DarkGray));
    assert_eq!(Palette::new(8).accent(Mode::Select).fg, Some(Color::Black));
    assert_eq!(
        Palette::new(16_777_216).accent(Mode::Delete).bg,
        Some(Color::Rgb(55, 6, 3))
    );
}

#[test]
fn scroll_offset_keeps_cursor_visible() {
    let mut state = State::new(
        (0..50)
            .map(|i| session(Agent::Pi, &format!("id-{i}")))
            .collect(),
        "/p/proj".into(),
    );
    state.cursor = 49;
    screen(&mut state, 100, 15);
    assert!(state.scroll_offset > 0);
    state.cursor = 0;
    screen(&mut state, 100, 15);
    assert_eq!(state.scroll_offset, 0);
}

#[test]
fn modified_confirmation_keys_do_not_delete_sessions() {
    for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
        let mut state = state();
        key(&mut state, KeyCode::Char('d'));
        key(&mut state, KeyCode::Tab);
        key(&mut state, KeyCode::Enter);
        let action = state.handle_key(KeyEvent::new(KeyCode::Char('y'), modifiers));
        assert_eq!(action, Action::None);
        assert!(!state.confirming);
    }
}

#[test]
fn filter_labels_have_their_agent_colors() {
    let mut state = state();
    let terminal = screen(&mut state, 180, 30);
    let buffer = terminal.backend().buffer();
    let line = (0..180)
        .map(|column| buffer[(column, 5)].symbol())
        .collect::<String>();
    let palette = Palette::new(16_777_216);
    for agent in Agent::ALL {
        let start = line.find(agent.label()).unwrap() as u16;
        assert_eq!(buffer[(start, 5)].fg, palette.agent(agent).fg.unwrap());
    }
}

#[test]
fn filter_bar_and_hints_show_directory_scope_and_new_mode_key() {
    let mut state = state();
    let select = text(&screen(&mut state, 180, 30));
    assert!(select.contains("F: CWD"));
    assert!(select.find("F: CWD").unwrap() < select.find("Agents:").unwrap());
    assert!(select.contains("D: Delete mode"));
    assert!(select.contains("F: CWD/All"));
    key(&mut state, KeyCode::Char('f'));
    key(&mut state, KeyCode::Char('d'));
    let delete = text(&screen(&mut state, 240, 30));
    assert!(delete.contains("F: All"));
    assert!(delete.contains("D: Select mode"));
    let narrow = text(&screen(&mut state, 20, 4));
    assert!(narrow.contains("F: All"));

    let mut empty = State::new(vec![], "/p/proj".into());
    let rendered = text(&screen(&mut empty, 100, 8));
    assert!(rendered.contains("F: CWD/All"));
    assert!(rendered.contains("D: Delete mode"));
    key(&mut empty, KeyCode::Char('d'));
    let rendered = text(&screen(&mut empty, 100, 8));
    assert!(rendered.contains("D: Select mode"));
}

#[test]
fn unnamed_details_keep_the_full_id_and_recent_commands() {
    let mut s = session(Agent::Pi, "aaaaaaaa-0000-0000-0000-000000000001");
    s.commands = [
        "first-cmd",
        "second-cmd",
        "third-cmd",
        "fourth-cmd",
        "fifth-cmd",
        "sixth-cmd",
    ]
    .map(String::from)
    .to_vec();
    let mut state = State::new(vec![s], "/p/proj".into());
    let terminal = screen(&mut state, 180, 40);
    let text = text(&terminal);
    assert!(!text.contains("Name:"));
    assert!(text.contains("aaaaaaaa-0000-0000-0000-000000000001"));
    assert!(text.contains("1. sixth-cmd"));
    assert!(text.contains("5. second-cmd"));
    assert!(!text.contains("first-cmd"));
}

#[test]
fn deletion_reloads_sessions_keeps_filters_and_counts_failed_deletes() {
    use agents_sessions::{data::agents::load_all_sessions, utils::CommandRunner};
    use serde_json::json;
    use std::{ffi::OsString, fs};
    struct FailingRunner;
    impl CommandRunner for FailingRunner {
        fn run(&self, _: &[OsString]) -> Result<(), String> {
            Err("cannot delete".into())
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = common::paths(tmp.path());
    common::write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[json!({"sessionId":"id","timestamp":1000,"project":"/p","display":"hi"})],
    );
    fs::create_dir_all(&paths.codex).unwrap();
    let db = rusqlite::Connection::open(paths.codex.join("state_1.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE threads (id TEXT,cwd TEXT,name TEXT,first_user_message TEXT,created_at_ms INTEGER,updated_at_ms INTEGER,archived INTEGER,thread_source TEXT);
        INSERT INTO threads VALUES ('x','/p',NULL,'hi',1000,2000,0,'user');
        INSERT INTO threads VALUES ('other','/elsewhere',NULL,'hi',1000,2000,0,'user');").unwrap();
    for (scope, count) in [(DirectoryScope::Cwd, 1), (DirectoryScope::All, 2)] {
        let mut state = State::new(load_all_sessions(&paths).unwrap(), "/p".into());
        if scope == DirectoryScope::All {
            key(&mut state, KeyCode::Char('f'));
        }
        key(&mut state, KeyCode::Char('2'));
        key(&mut state, KeyCode::Char('d'));
        key(&mut state, KeyCode::Tab);
        state.delete_selected(&paths, &FailingRunner).unwrap();
        assert_eq!(state.all_sessions.len(), 3);
        assert_eq!(state.visible.len(), count);
        assert_eq!(state.mode, Mode::Select);
        assert_eq!(state.directory_scope, scope);
        assert!(state.selected.is_empty());
        assert!(state.range_anchor.is_none());
        assert_eq!(state.filters, HashSet::from([Agent::Codex]));
        assert_eq!(
            state.message.as_deref(),
            Some("Deleted 0 session(s). 1 error(s): cannot delete")
        );
    }
}
