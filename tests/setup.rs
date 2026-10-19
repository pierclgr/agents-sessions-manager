use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    _temporary: tempfile::TempDir,
    repository: PathBuf,
    bin: PathBuf,
    tools: PathBuf,
    cwd: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("repository's folder");
        let bin = root.join("local bin");
        let tools = root.join("tools");
        let cwd = root.join("other directory");
        for path in [&repository, &tools, &cwd] {
            fs::create_dir_all(path).unwrap();
        }
        fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/setup.sh"),
            repository.join("setup.sh"),
        )
        .unwrap();
        fs::write(
            tools.join("cargo"),
            r#"#!/bin/sh
set -eu
if [ "${TEST_CARGO_FAIL:-0}" = 1 ]; then exit 9; fi
while [ "$#" -gt 0 ]; do
    if [ "$1" = --root ]; then shift; prefix=$1; fi
    shift
done
mkdir -p "$prefix/bin"
cp "$TEST_EXECUTABLE" "$prefix/bin/agents-sessions"
"#,
        )
        .unwrap();
        fs::set_permissions(tools.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _temporary: temporary,
            repository,
            bin,
            tools,
            cwd,
        }
    }

    fn setup(&self) -> Command {
        let mut command = Command::new("/bin/bash");
        let path = format!(
            "{}:{}",
            self.tools.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .arg(self.repository.join("setup.sh"))
            .arg(&self.bin)
            .current_dir(&self.cwd)
            .env("PATH", path)
            .env("TEST_EXECUTABLE", env!("CARGO_BIN_EXE_agents-sessions"));
        command
    }

    fn install(&self) -> Output {
        self.setup().output().unwrap()
    }

    fn run(&self, name: &str, args: &[&str]) -> Output {
        Command::new(self.bin.join(name))
            .args(args)
            .current_dir(&self.cwd)
            .env("CLAUDE_DATA_DIR", self.cwd.join("claude"))
            .env("CODEX_HOME", self.cwd.join("codex"))
            .env("XDG_DATA_HOME", self.cwd.join("data"))
            .env("PI_CODING_AGENT_SESSION_DIR", self.cwd.join("pi"))
            .output()
            .unwrap()
    }
}

#[test]
fn installs_a_regular_executable_and_relative_alias() {
    let fixture = Fixture::new();
    let output = fixture.install();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.bin.join("agents-sessions").is_symlink());
    assert_eq!(
        fs::read_link(fixture.bin.join("sessions")).unwrap(),
        PathBuf::from("agents-sessions")
    );
    for name in ["agents-sessions", "sessions"] {
        assert!(fixture.run(name, &["--help"]).status.success());
        let output = fixture.run(name, &["ls"]);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "No sessions found.\n"
        );
        for route in ["launch", "delete"] {
            assert_eq!(
                fixture.run(name, &[route, "missing"]).status.code(),
                Some(1)
            );
        }
    }
}

#[test]
fn standalone_commands_survive_removing_the_source_checkout_and_build_tools() {
    let fixture = Fixture::new();
    assert!(fixture.install().status.success());
    fs::remove_dir_all(&fixture.repository).unwrap();
    fs::remove_dir_all(&fixture.tools).unwrap();
    for name in ["agents-sessions", "sessions"] {
        let output = Command::new(fixture.bin.join(name))
            .arg("--help")
            .env("PATH", &fixture.bin)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
}

#[test]
fn repeated_installs_update_only_managed_files() {
    let fixture = Fixture::new();
    for _ in 0..2 {
        let output = fixture.install();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(fixture.run("sessions", &["--help"]).status.success());
}

#[test]
fn refuses_every_conflict_before_installing_either_command() {
    for name in ["agents-sessions", "sessions"] {
        for kind in ["file", "directory", "broken-link"] {
            let fixture = Fixture::new();
            fs::create_dir(&fixture.bin).unwrap();
            let conflict = fixture.bin.join(name);
            match kind {
                "file" => fs::write(&conflict, "keep me").unwrap(),
                "directory" => fs::create_dir(&conflict).unwrap(),
                _ => symlink("missing-target", &conflict).unwrap(),
            }
            let output = fixture.install();
            assert!(!output.status.success(), "{name} {kind}");
            assert!(String::from_utf8_lossy(&output.stderr).contains(conflict.to_str().unwrap()));
            let other = fixture.bin.join(if name == "sessions" {
                "agents-sessions"
            } else {
                "sessions"
            });
            assert!(!other.exists() && !other.is_symlink());
            if kind == "file" {
                assert_eq!(fs::read_to_string(&conflict).unwrap(), "keep me");
            }
            if kind == "broken-link" {
                assert_eq!(
                    fs::read_link(&conflict).unwrap(),
                    PathBuf::from("missing-target")
                );
            }
        }
    }
}

#[test]
fn failed_build_preserves_the_previous_installation() {
    let fixture = Fixture::new();
    assert!(fixture.install().status.success());
    let before = fs::read(fixture.bin.join("agents-sessions")).unwrap();
    let output = fixture
        .setup()
        .env("TEST_CARGO_FAIL", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read(fixture.bin.join("agents-sessions")).unwrap(),
        before
    );
    assert!(fixture.run("sessions", &["--help"]).status.success());
    assert_eq!(fs::read_dir(&fixture.bin).unwrap().count(), 3);
}

#[test]
fn modified_installed_binaries_and_aliases_are_not_overwritten() {
    for name in ["agents-sessions", "sessions"] {
        let fixture = Fixture::new();
        assert!(fixture.install().status.success());
        let path = fixture.bin.join(name);
        fs::remove_file(&path).unwrap();
        fs::write(&path, "user replacement").unwrap();
        assert!(!fixture.install().status.success());
        assert_eq!(fs::read_to_string(&path).unwrap(), "user replacement");
    }
}

#[test]
fn legacy_installer_links_can_be_upgraded() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.bin).unwrap();
    for name in ["agents-sessions", "sessions"] {
        symlink(
            fixture.repository.join("agents-sessions"),
            fixture.bin.join(name),
        )
        .unwrap();
    }
    let output = fixture.install();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.bin.join("agents-sessions").is_symlink());
}

#[test]
fn printed_path_line_and_chained_links_work() {
    let fixture = Fixture::new();
    let output = fixture.install();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let export = stdout
        .lines()
        .find(|line| line.starts_with("export PATH="))
        .unwrap();
    let output = Command::new("bash")
        .args([
            "-c",
            &format!("set -e\n{export}\ncommand -v agents-sessions\ncommand -v sessions"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [
            fixture.bin.join("agents-sessions").to_str().unwrap(),
            fixture.bin.join("sessions").to_str().unwrap()
        ]
    );
    symlink("sessions", fixture.bin.join("shortcut")).unwrap();
    assert!(fixture.run("shortcut", &["--help"]).status.success());
}

#[test]
fn failed_replacement_rolls_back_both_commands_and_metadata() {
    let fixture = Fixture::new();
    assert!(fixture.install().status.success());
    let before = fs::read(fixture.bin.join("agents-sessions")).unwrap();
    let metadata = fs::read(fixture.bin.join(".agents-sessions-install")).unwrap();
    fs::write(
        fixture.tools.join("mv"),
        r#"#!/bin/sh
set -eu
case "$2" in
    */new-sessions) exit 8 ;;
esac
exec /bin/mv "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(fixture.tools.join("mv"), fs::Permissions::from_mode(0o755)).unwrap();
    let output = fixture.install();
    assert!(!output.status.success());
    assert_eq!(
        fs::read(fixture.bin.join("agents-sessions")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(fixture.bin.join(".agents-sessions-install")).unwrap(),
        metadata
    );
    assert_eq!(
        fs::read_link(fixture.bin.join("sessions")).unwrap(),
        PathBuf::from("agents-sessions")
    );
    assert!(fixture.run("sessions", &["--help"]).status.success());
    assert_eq!(fs::read_dir(&fixture.bin).unwrap().count(), 3);
}

#[test]
fn missing_cargo_has_a_clear_error_without_installing_commands() {
    let fixture = Fixture::new();
    let isolated_tools = fixture.tools.join("without-cargo");
    fs::create_dir(&isolated_tools).unwrap();
    symlink("/usr/bin/dirname", isolated_tools.join("dirname")).unwrap();
    let output = fixture
        .setup()
        .env("PATH", isolated_tools)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Rust and Cargo are required"));
    assert!(!fixture.bin.exists());
}
