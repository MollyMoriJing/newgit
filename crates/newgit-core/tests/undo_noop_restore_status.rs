//! An undo puts back what a checkpoint holds; it does not bring a resource
//! up. A restore that runs nothing — `[restore] mode = "none"` or
//! `"external"`, or no `[restore]` at all — leaves its resource exactly as
//! it found it, and the binding record has to say the same. Recording
//! `ready` because nothing failed turned a resource whose `prepare` failed,
//! or never ran because a dependency blocked it, into one that `status`,
//! readiness and every later blocked-dependency check took to be up.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use newgit_core::SourceSubstrate;
use newgit_core::branch::ResourceStatus;
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

fn setup(temp: &Utf8Path) -> MetadataStore {
    let repo = temp.join("store");
    std::fs::create_dir_all(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("write");
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

fn manager(store: MetadataStore) -> BranchManager {
    BranchManager::open(store).expect("manager")
}

fn write_resource(store: &MetadataStore, name: &str, contents: &str) {
    std::fs::write(
        store.paths().resources.join(format!("{name}.toml")),
        contents,
    )
    .expect("write resource");
}

/// A `prepare` that always fails, followed by whatever checkpoint and
/// restore sections the test hands in.
fn never_prepares(sections: &str) -> String {
    format!(
        r#"ownership = "workspace"

[actions.prepare]
command = "echo the install failed; exit 1"
{sections}"#
    )
}

/// The three ways to declare a restore that runs no command.
const NO_RESTORE: &str = "";
const RESTORE_NONE: &str = r#"
[restore]
mode = "none"
"#;
const RESTORE_EXTERNAL: &str = r#"
[checkpoint]
mode = "external"
state_ref = "preview-{{branch.slug}}"

[restore]
mode = "external"
"#;

/// Bound to a resource that never comes up: its own `prepare` would
/// succeed, but it never gets to run.
const DEPENDENT: &str = r#"ownership = "workspace"
depends_on = ["deps"]

[actions.prepare]
command = "touch dependent-prepared"
"#;

fn status_of(manager: &BranchManager, instance: &str, resource: &str) -> ResourceStatus {
    manager
        .store()
        .find_branch(instance)
        .expect("instance")
        .resources[resource]
        .status
}

/// The resource's cell in `newgit status`.
fn reported_state(manager: &BranchManager, instance: &str, resource: &str) -> String {
    manager
        .statuses()
        .expect("statuses")
        .into_iter()
        .find(|report| report.branch.name == instance)
        .expect("instance report")
        .resources
        .into_iter()
        .find(|report| report.name == resource)
        .expect("resource report")
        .state
}

#[test]
fn a_restore_that_runs_nothing_leaves_a_failed_prepare_failed() {
    let (_guard, temp) = tempdir();
    let store = setup(&temp);
    write_resource(&store, "no-restore", &never_prepares(NO_RESTORE));
    write_resource(&store, "restore-none", &never_prepares(RESTORE_NONE));
    write_resource(
        &store,
        "restore-external",
        &never_prepares(RESTORE_EXTERNAL),
    );
    let m = manager(store);
    let spawned = m.spawn("feature-a", None).expect("spawn");
    let mut failed = spawned.failed_resources();
    failed.sort_unstable();
    assert_eq!(
        failed,
        vec!["no-restore", "restore-external", "restore-none"]
    );
    m.checkpoint("feature-a", None).expect("checkpoint");

    let undo = m.undo("feature-a", &UndoOptions::default()).expect("undo");
    // Not a failed undo: none of these restores has anything to fail, and
    // the undo did all it set out to do. It just never set out to run
    // `prepare`.
    assert!(undo.is_complete(), "{:?}", undo.resources);
    // Its safety checkpoint skipped them as known-broken, and the reason it
    // gives covers a `prepare` — no restore had ever run here to blame.
    assert!(
        undo.warnings.iter().any(|warning| {
            warning.contains("`no-restore` is in a failed state") && warning.contains("prepare")
        }),
        "{:?}",
        undo.warnings
    );

    for name in ["no-restore", "restore-none", "restore-external"] {
        assert_eq!(
            status_of(&m, "feature-a", name),
            ResourceStatus::Failed,
            "`{name}`: no command ran, so nothing brought it up"
        );
        assert_eq!(reported_state(&m, "feature-a", name), "failed", "`{name}`");
    }
    let branch = m.store().find_branch("feature-a").expect("instance");
    let readiness = m.readiness(&branch);
    assert!(
        readiness
            .problems
            .iter()
            .any(|problem| problem.contains("`no-restore` failed")),
        "{:?}",
        readiness.problems
    );
}

/// A dependent whose `prepare` never ran is `pending` going in, and an undo
/// that runs nothing for it — and leaves its dependency failed — must not
/// come out claiming it is ready: its blocker has not cleared.
#[test]
fn a_restore_that_runs_nothing_leaves_a_blocked_dependent_blocked() {
    let (_guard, temp) = tempdir();
    let store = setup(&temp);
    write_resource(&store, "deps", &never_prepares(NO_RESTORE));
    write_resource(&store, "dependent", DEPENDENT);
    let m = manager(store);
    let spawned = m.spawn("feature-a", None).expect("spawn");
    let dependent = spawned
        .resources
        .iter()
        .find(|resource| resource.name == "dependent")
        .expect("dependent");
    assert_eq!(dependent.status, ResourceStatus::Pending);
    assert_eq!(dependent.blocked_by, vec!["deps"]);
    m.checkpoint("feature-a", None).expect("checkpoint");

    m.undo("feature-a", &UndoOptions::default()).expect("undo");

    assert_eq!(status_of(&m, "feature-a", "deps"), ResourceStatus::Failed);
    assert_eq!(
        status_of(&m, "feature-a", "dependent"),
        ResourceStatus::Pending,
        "its prepare has still never run"
    );
    assert_eq!(
        reported_state(&m, "feature-a", "dependent"),
        "blocked(deps)"
    );
    let branch = m.store().find_branch("feature-a").expect("instance");
    assert!(!m.readiness(&branch).is_ready());
}

/// The other half of the rule, unchanged: a restore that runs a command and
/// succeeds is what brings a resource back, whatever state it was in. Here
/// `recompute` re-runs a `prepare` that failed at spawn and works now.
#[test]
fn a_restore_that_runs_and_succeeds_still_brings_a_failed_resource_up() {
    let (_guard, temp) = tempdir();
    let store = setup(&temp);
    let fail_once = temp.join("failed-once");
    write_resource(
        &store,
        "deps",
        &format!(
            r#"ownership = "workspace"

[actions.prepare]
command = "test -f {fail_once} || (touch {fail_once} && exit 1)"

[restore]
mode = "recompute"
"#
        ),
    );
    let m = manager(store);
    let spawned = m.spawn("feature-a", None).expect("spawn");
    assert_eq!(spawned.failed_resources(), vec!["deps"]);
    m.checkpoint("feature-a", None).expect("checkpoint");

    let undo = m.undo("feature-a", &UndoOptions::default()).expect("undo");

    assert!(undo.is_complete(), "{:?}", undo.resources);
    assert_eq!(status_of(&m, "feature-a", "deps"), ResourceStatus::Ready);
    assert_eq!(reported_state(&m, "feature-a", "deps"), "ready");
}
