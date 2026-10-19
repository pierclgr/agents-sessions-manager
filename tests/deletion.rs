mod common;

use agents_sessions::{
    data::{Agent, agents::provider},
    utils::CommandRunner,
};
use common::{paths, session, write_jsonl};
use serde_json::json;
use std::{cell::RefCell, ffi::OsString, fs, os::unix::fs::symlink};

const A: &str = "aaaaaaaa-0000-0000-0000-000000000001";
const B: &str = "bbbbbbbb-0000-0000-0000-000000000002";
const C: &str = "cccccccc-0000-0000-0000-000000000003";

struct Runner {
    fail: bool,
    calls: RefCell<Vec<Vec<OsString>>>,
}

impl CommandRunner for Runner {
    fn run(&self, argv: &[OsString]) -> Result<(), String> {
        self.calls.borrow_mut().push(argv.to_vec());
        if self.fail {
            Err("unpushed commits".into())
        } else {
            Ok(())
        }
    }
}

fn fixture(root: &std::path::Path) -> agents_sessions::data::DataPaths {
    let paths = paths(root);
    write_jsonl(
        &paths.claude.join("history.jsonl"),
        &[
            json!({"sessionId":A,"pastedContents":{"1":{"contentHash":"only-a"}}}),
            json!({"sessionId":A,"pastedContents":{"1":{"contentHash":"shared"}}}),
            json!({"sessionId":B,"pastedContents":{"1":{"contentHash":"shared"}}}),
            json!({"sessionId":C}),
        ],
    );
    for (id, project, slug) in [
        (A, "-p-my-proj-x", "shared-plan"),
        (B, "-p-my-proj-x", "shared-plan"),
        (C, "-p-other", "only-c"),
    ] {
        write_jsonl(
            &paths.claude.join(format!("projects/{project}/{id}.jsonl")),
            &[json!({"slug":slug})],
        );
    }
    write_jsonl(
        &paths
            .claude
            .join(format!("projects/-p-my-proj-x/{A}/subagents/agent-1.jsonl")),
        &[json!({})],
    );
    for directory in [
        format!("file-history/{A}"),
        format!("session-env/{A}"),
        "projects/-p-my-proj-x/memory".into(),
        "plans".into(),
        "paste-cache".into(),
    ] {
        fs::create_dir_all(paths.claude.join(directory)).unwrap();
    }
    for file in [
        "plans/shared-plan.md",
        "plans/only-c.md",
        "paste-cache/only-a.txt",
        "paste-cache/shared.txt",
        "projects/-p-my-proj-x/memory/MEMORY.md",
    ] {
        fs::write(paths.claude.join(file), "keep or delete").unwrap();
    }
    paths
}

#[test]
fn claude_deletes_artifacts_and_unused_pastes_but_keeps_shared_data() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let runner = Runner {
        fail: false,
        calls: RefCell::new(vec![]),
    };
    let errors =
        provider(Agent::Claude).delete_sessions(&[&session(Agent::Claude, A)], &paths, &runner);
    assert!(errors.is_empty(), "{errors:?}");
    for file in [
        format!("projects/-p-my-proj-x/{A}.jsonl"),
        format!("projects/-p-my-proj-x/{A}"),
        format!("file-history/{A}"),
        format!("session-env/{A}"),
        "paste-cache/only-a.txt".into(),
    ] {
        assert!(!paths.claude.join(file).exists());
    }
    for file in [
        "plans/shared-plan.md",
        "paste-cache/shared.txt",
        "projects/-p-my-proj-x/memory/MEMORY.md",
    ] {
        assert!(paths.claude.join(file).exists());
    }
    assert!(runner.calls.borrow().is_empty());
    assert!(
        !fs::read_to_string(paths.claude.join("history.jsonl"))
            .unwrap()
            .contains(A)
    );
}

#[test]
fn claude_removes_last_shared_plan_and_keeps_project_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let runner = Runner {
        fail: false,
        calls: RefCell::new(vec![]),
    };
    assert!(
        provider(Agent::Claude)
            .delete_sessions(
                &[&session(Agent::Claude, A), &session(Agent::Claude, B)],
                &paths,
                &runner
            )
            .is_empty()
    );
    assert!(!paths.claude.join("plans/shared-plan.md").exists());
    assert!(
        paths
            .claude
            .join("projects/-p-my-proj-x/memory/MEMORY.md")
            .exists()
    );
}

#[test]
fn claude_removes_empty_project_and_unshared_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let runner = Runner {
        fail: false,
        calls: RefCell::new(vec![]),
    };
    assert!(
        provider(Agent::Claude)
            .delete_sessions(&[&session(Agent::Claude, C)], &paths, &runner)
            .is_empty()
    );
    assert!(!paths.claude.join("projects/-p-other").exists());
    assert!(!paths.claude.join("plans/only-c.md").exists());
}

#[test]
fn background_command_failure_keeps_the_entire_session() {
    for fail in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let paths = fixture(tmp.path());
        write_jsonl(
            &paths.claude.join("jobs/cccccccc/state.json"),
            &[json!({"sessionId":C})],
        );
        let runner = Runner {
            fail,
            calls: RefCell::new(vec![]),
        };
        let errors = provider(Agent::Claude).delete_sessions(
            &[&session(Agent::Claude, C), &session(Agent::Claude, A)],
            &paths,
            &runner,
        );
        assert_eq!(errors.len(), usize::from(fail));
        assert_eq!(
            runner.calls.borrow()[0],
            ["claude", "rm", "cccccccc"].map(OsString::from)
        );
        assert_eq!(
            paths
                .claude
                .join(format!("projects/-p-other/{C}.jsonl"))
                .exists(),
            fail
        );
        assert_eq!(
            fs::read_to_string(paths.claude.join("history.jsonl"))
                .unwrap()
                .contains(C),
            fail
        );
        assert!(
            !paths
                .claude
                .join(format!("projects/-p-my-proj-x/{A}.jsonl"))
                .exists()
        );
    }
}

#[test]
fn failed_history_rewrite_keeps_session_files() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    fs::remove_file(paths.claude.join("history.jsonl")).unwrap();
    fs::create_dir(paths.claude.join("history.jsonl")).unwrap();
    let runner = Runner {
        fail: false,
        calls: RefCell::new(vec![]),
    };
    let errors =
        provider(Agent::Claude).delete_sessions(&[&session(Agent::Claude, A)], &paths, &runner);
    assert_eq!(errors.len(), 1);
    assert!(
        paths
            .claude
            .join(format!("projects/-p-my-proj-x/{A}.jsonl"))
            .exists()
    );
}

#[test]
fn external_links_are_never_traversed_or_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let external = outside.path().join(format!("{A}.txt"));
    fs::write(&external, "outside").unwrap();
    symlink(outside.path(), paths.claude.join("linked-directory")).unwrap();
    symlink(&external, paths.claude.join(format!("{A}-linked-file"))).unwrap();
    symlink(outside.path(), paths.claude.join("projects/linked-project")).unwrap();
    let runner = Runner {
        fail: false,
        calls: RefCell::new(vec![]),
    };
    assert!(
        provider(Agent::Claude)
            .delete_sessions(&[&session(Agent::Claude, A)], &paths, &runner)
            .is_empty()
    );
    assert_eq!(fs::read_to_string(external).unwrap(), "outside");
}
