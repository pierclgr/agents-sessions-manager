mod common;

use agents_sessions::{
    data::{Agent, agents::provider},
    utils::CommandRunner,
};
use common::{paths, session, write_jsonl};
use serde_json::json;
use std::{
    cell::RefCell,
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

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

#[test]
fn claude_batch_keeps_a_plan_when_a_transcript_cannot_be_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let blocked_project = paths.claude.join("projects/-p-other");
    write_jsonl(
        &blocked_project.join(format!("{C}.jsonl")),
        &[json!({"slug":"shared-plan"})],
    );
    let permissions = fs::metadata(&blocked_project).unwrap().permissions();
    fs::set_permissions(&blocked_project, fs::Permissions::from_mode(0o555)).unwrap();
    let errors = provider(Agent::Claude).delete_sessions(
        &[
            &session(Agent::Claude, C),
            &session(Agent::Claude, A),
            &session(Agent::Claude, B),
        ],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    fs::set_permissions(&blocked_project, permissions).unwrap();
    assert!(!errors.is_empty());
    assert!(blocked_project.join(format!("{C}.jsonl")).is_file());
    assert!(paths.claude.join("plans/shared-plan.md").is_file());
    for id in [A, B] {
        assert!(
            !paths
                .claude
                .join(format!("projects/-p-my-proj-x/{id}.jsonl"))
                .exists()
        );
    }
}

#[test]
fn claude_batch_tracks_unreadable_plan_references_until_removed() {
    for delete_unreadable in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let paths = fixture(tmp.path());
        let unreadable = paths.claude.join(format!("projects/-p-other/{C}.jsonl"));
        let permissions = fs::metadata(&unreadable).unwrap().permissions();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let mut sessions = vec![session(Agent::Claude, A)];
        if delete_unreadable {
            sessions.push(session(Agent::Claude, C));
        }
        sessions.push(session(Agent::Claude, B));
        let selected: Vec<_> = sessions.iter().collect();
        let errors = provider(Agent::Claude).delete_sessions(
            &selected,
            &paths,
            &Runner {
                fail: false,
                calls: RefCell::new(vec![]),
            },
        );
        if unreadable.exists() {
            fs::set_permissions(&unreadable, permissions).unwrap();
        }
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            paths.claude.join("plans/shared-plan.md").exists(),
            !delete_unreadable
        );
    }
}

#[test]
fn claude_batch_preserves_secondary_slug_references_and_first_slug_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    write_jsonl(
        &paths
            .claude
            .join(format!("projects/-p-my-proj-x/{B}.jsonl")),
        &[json!({"slug":"other-plan"}), json!({"slug":"shared-plan"})],
    );
    fs::write(paths.claude.join("plans/other-plan.md"), "other").unwrap();
    let errors = provider(Agent::Claude).delete_sessions(
        &[&session(Agent::Claude, A), &session(Agent::Claude, B)],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert!(paths.claude.join("plans/shared-plan.md").exists());
    assert!(!paths.claude.join("plans/other-plan.md").exists());
}

#[test]
fn claude_batch_notices_plan_references_removed_with_an_artifact_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = fixture(tmp.path());
    let directory = paths.claude.join(format!("projects/project-{B}"));
    fs::create_dir(&directory).unwrap();
    for id in [B, C] {
        fs::rename(
            paths.claude.join(if id == B {
                format!("projects/-p-my-proj-x/{id}.jsonl")
            } else {
                format!("projects/-p-other/{id}.jsonl")
            }),
            directory.join(format!("{id}.jsonl")),
        )
        .unwrap();
        write_jsonl(
            &directory.join(format!("{id}.jsonl")),
            &[json!({"slug":"shared-plan"})],
        );
    }
    let errors = provider(Agent::Claude).delete_sessions(
        &[&session(Agent::Claude, A), &session(Agent::Claude, B)],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert!(!directory.exists());
    assert!(!paths.claude.join("plans/shared-plan.md").exists());
}

#[test]
fn claude_batch_refreshes_jobs_after_external_commands() {
    struct ChangingJobs {
        root: PathBuf,
        calls: RefCell<Vec<OsString>>,
        fail_first: bool,
    }
    impl CommandRunner for ChangingJobs {
        fn run(&self, argv: &[OsString]) -> Result<(), String> {
            self.calls.borrow_mut().push(argv[2].clone());
            if self.calls.borrow().len() == 1 {
                fs::remove_dir_all(self.root.join("jobs/a-job")).unwrap();
                write_jsonl(
                    &self.root.join("jobs/z-job/state.json"),
                    &[json!({"sessionId":C})],
                );
                fs::create_dir_all(self.root.join(format!("session-env/{C}"))).unwrap();
                if self.fail_first {
                    return Err("background removal failed".into());
                }
            }
            Ok(())
        }
    }
    for fail_first in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let paths = fixture(tmp.path());
        write_jsonl(
            &paths.claude.join("jobs/a-job/state.json"),
            &[json!({"sessionId":A})],
        );
        write_jsonl(
            &paths.claude.join("jobs/z-job/state.json"),
            &[json!({"sessionId":B})],
        );
        let runner = ChangingJobs {
            root: paths.claude.clone(),
            calls: RefCell::new(vec![]),
            fail_first,
        };
        let errors = provider(Agent::Claude).delete_sessions(
            &[&session(Agent::Claude, A), &session(Agent::Claude, C)],
            &paths,
            &runner,
        );
        assert_eq!(errors.len(), usize::from(fail_first));
        assert_eq!(
            *runner.calls.borrow(),
            ["a-job", "z-job"].map(OsString::from)
        );
        assert_eq!(
            paths
                .claude
                .join(format!("projects/-p-my-proj-x/{A}.jsonl"))
                .exists(),
            fail_first
        );
        assert!(!paths.claude.join(format!("session-env/{C}")).exists());
    }
}

#[test]
fn claude_batch_keeps_substring_matching_and_repeated_session_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let ids = ["session-1", "session-10", "keep"];
    let history: Vec<_> = ids.iter().map(|id| json!({"sessionId":id})).collect();
    write_jsonl(&paths.claude.join("history.jsonl"), &history);
    for id in ids {
        write_jsonl(
            &paths.claude.join(format!("projects/project/{id}.jsonl")),
            &[json!({"slug":format!("plan-{id}")})],
        );
        write_jsonl(&paths.claude.join(format!("plans/plan-{id}.md")), &[]);
    }
    let errors = provider(Agent::Claude).delete_sessions(
        &[
            &session(Agent::Claude, "session-1"),
            &session(Agent::Claude, "session-10"),
            &session(Agent::Claude, "session-1"),
        ],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    assert!(errors.is_empty(), "{errors:?}");
    for id in ["session-1", "session-10"] {
        assert!(
            !paths
                .claude
                .join(format!("projects/project/{id}.jsonl"))
                .exists()
        );
        assert!(!paths.claude.join(format!("plans/plan-{id}.md")).exists());
    }
    assert!(paths.claude.join("projects/project/keep.jsonl").exists());
    assert!(paths.claude.join("plans/plan-keep.md").exists());
    assert_eq!(
        fs::read_to_string(paths.claude.join("history.jsonl")).unwrap(),
        "{\"sessionId\":\"keep\"}\n"
    );
}

#[test]
fn pi_batch_keeps_suffix_matching_order_and_duplicate_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    for filename in [
        "a-first/001_x_a.jsonl",
        "a-first/002_a.jsonl",
        "z-last/001_x_a.jsonl",
        "z-last/003_keep.jsonl",
    ] {
        write_jsonl(&paths.pi_sessions.join(filename), &[json!({})]);
    }
    let sessions: Vec<_> = ["a", "x_a", "a", "a"]
        .iter()
        .map(|id| session(Agent::Pi, id))
        .collect();
    let selected: Vec<_> = sessions.iter().collect();
    let errors = provider(Agent::Pi).delete_sessions(
        &selected,
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    assert_eq!(errors, ["Pi session file not found for a"]);
    assert!(!paths.pi_sessions.join("a-first").exists());
    assert!(!paths.pi_sessions.join("z-last/001_x_a.jsonl").exists());
    assert!(paths.pi_sessions.join("z-last/003_keep.jsonl").is_file());
}

#[test]
fn pi_batch_retries_the_first_matching_file_after_deletion_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    for filename in [
        "a-first/001_id.jsonl",
        "z-last/001_id.jsonl",
        "z-last/002_other.jsonl",
    ] {
        write_jsonl(&paths.pi_sessions.join(filename), &[json!({})]);
    }
    let blocked_project = paths.pi_sessions.join("a-first");
    let permissions = fs::metadata(&blocked_project).unwrap().permissions();
    fs::set_permissions(&blocked_project, fs::Permissions::from_mode(0o555)).unwrap();
    let errors = provider(Agent::Pi).delete_sessions(
        &[
            &session(Agent::Pi, "id"),
            &session(Agent::Pi, "id"),
            &session(Agent::Pi, "other"),
        ],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    fs::set_permissions(&blocked_project, permissions).unwrap();
    assert_eq!(errors.len(), 2);
    assert!(
        errors
            .iter()
            .all(|error| error.contains("a-first/001_id.jsonl"))
    );
    assert!(blocked_project.join("001_id.jsonl").is_file());
    assert!(paths.pi_sessions.join("z-last/001_id.jsonl").is_file());
    assert!(!paths.pi_sessions.join("z-last/002_other.jsonl").exists());
}

#[test]
fn pi_batch_rechecks_links_after_target_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let paths = paths(tmp.path());
    let target = paths.pi_sessions.join("a-first/001_target.jsonl");
    write_jsonl(&target, &[json!({})]);
    fs::create_dir(paths.pi_sessions.join("z-last")).unwrap();
    symlink(&target, paths.pi_sessions.join("z-last/002_alias.jsonl")).unwrap();
    write_jsonl(&outside.path().join("003_external.jsonl"), &[json!({})]);
    symlink(
        outside.path().join("003_external.jsonl"),
        paths.pi_sessions.join("z-last/003_external.jsonl"),
    )
    .unwrap();
    symlink(outside.path(), paths.pi_sessions.join("00-linked-project")).unwrap();
    let errors = provider(Agent::Pi).delete_sessions(
        &[
            &session(Agent::Pi, "target"),
            &session(Agent::Pi, "alias"),
            &session(Agent::Pi, "external"),
        ],
        &paths,
        &Runner {
            fail: false,
            calls: RefCell::new(vec![]),
        },
    );
    assert_eq!(
        errors,
        [
            "Pi session file not found for alias",
            "Pi session file not found for external"
        ]
    );
    assert!(outside.path().join("003_external.jsonl").is_file());
}
