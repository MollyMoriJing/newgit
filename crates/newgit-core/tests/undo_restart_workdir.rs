//! Regression test for #109: `undo` restarted a resource's long-running
//! action from the workspace root, ignoring its `workdir`, so a dev server
//! that `newgit action web.start` ran in `apps/web` came back somewhere else
//! after an undo — and looked healthy, since the supervisor only tracks
//! liveness.
//!
//! New tests live in their own file per AGENTS.md's "Working in parallel"
//! conventions, rather than appended to an existing integration test file.

use std::process::Command;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use newgit_core::SourceSubstrate;
use newgit_core::config::WorkspaceSection;
use newgit_core::manager::{BranchManager, UndoOptions};
use newgit_core::store::MetadataStore;

fn git(dir: &Utf8Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir.as_str())
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A store whose committed tree has the directory the action runs in.
fn setup(temp: &Utf8Path) -> MetadataStore {
    let repo = temp.join("store");
    std::fs::create_dir_all(repo.join("packages/app")).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("write");
    std::fs::write(repo.join("packages/app/.keep"), "").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);

    let store = MetadataStore::init(&repo, "proj", SourceSubstrate::Git).expect("init");
    let mut config = store.load_config().expect("config");
    config.workspace = Some(WorkspaceSection {
        root: Some(temp.join("workspaces")),
        materializer: None,
    });
    store.write_config(&config).expect("write config");
    store
}

fn tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(temp.path().canonicalize().expect("canonicalize"))
        .expect("utf8 tempdir");
    (temp, path)
}

fn write_resource(store: &MetadataStore, name: &str, contents: &str) {
    std::fs::write(
        store.paths().resources.join(format!("{name}.toml")),
        contents,
    )
    .expect("write resource");
}

/// The directories the action has started in so far, one per start, waiting
/// up to five seconds for the `count`th to be written.
fn starts(witness: &Utf8Path, count: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let lines: Vec<String> = std::fs::read_to_string(witness)
            .unwrap_or_default()
            .lines()
            .map(ToOwned::to_owned)
            .collect();
        if lines.len() >= count || Instant::now() >= deadline {
            return lines;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn undo_restarts_a_long_running_action_in_its_workdir() {
    let (_guard, temp) = tempdir();
    let store = setup(&temp);
    let witness = temp.join("cwd.txt");
    // On the action rather than the resource: the restart has to ask the
    // action, not only the resource-level default.
    write_resource(
        &store,
        "app",
        &format!(
            r#"ownership = "branch"

[actions.start]
command = "pwd >> {witness}; sleep 30"
long_running = true
workdir = "packages/app"

[actions.stop]
signal = "term"
"#
        ),
    );
    let m = BranchManager::open(store).expect("manager");
    let workspace = m
        .spawn("feature-a", None)
        .expect("spawn")
        .branch
        .workspace_path;
    let expected = workspace.join("packages/app");

    m.run_action("feature-a", "app.start").expect("start");
    assert_eq!(
        starts(&witness, 1),
        [expected.as_str()],
        "`newgit action app.start` runs in the workdir"
    );

    let checkpoint = m.checkpoint("feature-a", None).expect("checkpoint");
    assert!(checkpoint.record.resource_states[0].was_running);
    let undone = m.undo("feature-a", &UndoOptions::default()).expect("undo");
    assert!(
        undone.resources[0].action.ends_with("+ restarted"),
        "{:?}",
        undone.resources
    );

    let after_undo = starts(&witness, 2);
    m.run_action("feature-a", "app.stop").expect("stop");
    assert_eq!(
        after_undo,
        [expected.as_str(), expected.as_str()],
        "the restart after undo ran somewhere other than the workdir"
    );
}
