mod common;

use common::{paths, write_jsonl};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Browser {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    text: String,
    original_flags: String,
    finished: bool,
}

impl Browser {
    fn start(root: &Path, args: &[&str], path: Option<&Path>) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 35,
                cols: 180,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let original_flags = format!("{:?}", pair.master.get_termios().unwrap().local_flags);
        let paths = paths(root);
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_agents-sessions"));
        command.args(args);
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("CLAUDE_DATA_DIR", paths.claude);
        command.env("CODEX_HOME", paths.codex);
        command.env("XDG_DATA_HOME", root.join("data"));
        command.env("PI_CODING_AGENT_SESSION_DIR", paths.pi_sessions);
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            master: pair.master,
            child,
            writer,
            output,
            text: String::new(),
            original_flags,
            finished: false,
        }
    }

    fn wait_for(&mut self, expected: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while !self.text.contains(expected) {
            let bytes = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("terminal did not show {expected:?}: {}", self.text));
            self.text.push_str(&String::from_utf8_lossy(&bytes));
        }
    }

    fn input(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    fn finish(&mut self, exit_code: u32, alternate_screen: bool) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.finished = true;
                assert_eq!(status.exit_code(), exit_code, "{}", self.text);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "terminal process did not exit: {}",
                self.text
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        while let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(50)) {
            self.text.push_str(&String::from_utf8_lossy(&bytes));
        }
        assert_eq!(
            format!("{:?}", self.master.get_termios().unwrap().local_flags),
            self.original_flags
        );
        if alternate_screen {
            assert!(
                self.text.contains("\x1b[?1049l"),
                "alternate screen was not restored"
            );
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn history(root: &Path, project: &Path) {
    write_jsonl(
        &paths(root).claude.join("history.jsonl"),
        &[
            json!({"sessionId":"terminal-session","timestamp":1000,"project":project,"display":"first"}),
        ],
    );
}

#[test]
fn browser_navigation_resize_and_delete_cancellation_restore_the_terminal() {
    let root = tempfile::tempdir().unwrap();
    history(root.path(), root.path());
    let mut browser = Browser::start(root.path(), &[], None);
    browser.wait_for("MODE: SELECT");
    browser.input("d \r");
    browser.wait_for("Delete 1 session(s)?");
    browser.input("\x1b");
    browser
        .master
        .resize(PtySize {
            rows: 8,
            cols: 50,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    browser.input("jkdq");
    browser.finish(0, true);
    assert!(
        fs::read_to_string(paths(root.path()).claude.join("history.jsonl"))
            .unwrap()
            .contains("terminal-session")
    );
}

#[test]
fn empty_browser_quits_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let mut browser = Browser::start(root.path(), &[], None);
    browser.wait_for("found.");
    browser.input("q");
    browser.finish(0, true);
}

#[test]
fn raw_control_c_restores_the_terminal() {
    let root = tempfile::tempdir().unwrap();
    let mut browser = Browser::start(root.path(), &[], None);
    browser.wait_for("found.");
    browser.input("\x03");
    browser.finish(0, true);
}

#[test]
fn tui_launch_restores_terminal_before_the_agent_runs() {
    let root = tempfile::tempdir().unwrap();
    history(root.path(), root.path());
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(
        bin.join("claude"),
        "#!/bin/sh\nprintf 'AGENT_STARTED\\n'\nexit 7\n",
    )
    .unwrap();
    fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut browser = Browser::start(root.path(), &[], Some(&bin));
    browser.wait_for("MODE: SELECT");
    browser.input("\r");
    browser.wait_for("AGENT_STARTED");
    browser.finish(7, true);
    assert!(
        browser.text.find("\x1b[?1049l").unwrap() < browser.text.find("AGENT_STARTED").unwrap()
    );
}

#[test]
fn failed_tui_launch_restores_the_terminal() {
    let root = tempfile::tempdir().unwrap();
    history(root.path(), root.path());
    let mut browser = Browser::start(root.path(), &[], Some(root.path()));
    browser.wait_for("MODE: SELECT");
    browser.input("\r");
    browser.finish(1, true);
    assert!(browser.text.contains("Error: claude:"));
}

#[test]
fn cli_control_c_aborts_confirmation_without_deleting() {
    let root = tempfile::tempdir().unwrap();
    history(root.path(), root.path());
    let mut browser = Browser::start(root.path(), &["delete", "terminal-session"], None);
    browser.wait_for("Delete this session?");
    browser.input("\x03");
    browser.wait_for("Aborted.");
    browser.finish(0, false);
    assert!(
        fs::read_to_string(paths(root.path()).claude.join("history.jsonl"))
            .unwrap()
            .contains("terminal-session")
    );
}
