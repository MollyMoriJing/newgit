//! `root = "<tracker>"` on export targets (#141): the view rooted at that
//! tracker, its files at the top. While it mounts no tracker in Git that view
//! is its own history, so the first export of an imported tracker is a no-op
//! fast-forward of its upstream.

mod imports;

use imports::*;
use newgit_core::manager::RemoteExportOptions;

const TARGET: &str = "t3code";

fn rooted(store: &newgit_core::store::MetadataStore, bare: &camino::Utf8Path) {
    set_export(
        store,
        "t3code",
        Some(&export_to(bare, TARGET, "branch = \"newgit\"")),
    );
}

#[test]
fn the_first_export_of_an_import_is_a_no_op_fast_forward_of_its_upstream() {
    let (_guard, temp) = tempdir();
    let upstream = upstream(&temp);
    let store = project(&temp);
    let github = github_copy(&temp, &upstream, "t3code");
    let before = git(&github, &["rev-parse", "newgit"]);
    let outcome = import(&store, &upstream);
    rooted(&store, &github);

    let exported = manager(&store)
        .export_to_remote(TARGET, RemoteExportOptions::default())
        .expect("export");
    assert_eq!(exported.tip, outcome.main);
    assert_eq!(
        git(&github, &["rev-parse", "newgit"]),
        before,
        "ids intact: nothing moved"
    );

    // Work made here goes out as t3code's own commits, files at the top.
    let workspace = manager(&store)
        .spawn("feature", None)
        .expect("spawn")
        .branch
        .workspace_path;
    commit_as(
        &workspace,
        "ana",
        1_700_000_000,
        &[("apps/t3code/src/new.ts", "new\n")],
        "t3code: new",
    );
    manager(&store).merge("feature", "main").expect("merge");
    manager(&store)
        .export_to_remote(TARGET, RemoteExportOptions::default())
        .expect("export again");
    assert_eq!(
        git(&github, &["log", "-1", "--format=%s", "newgit"]),
        "t3code: new"
    );
    assert_eq!(
        git(&github, &["rev-parse", "newgit^"]),
        before,
        "a fast-forward"
    );
    let files = git(&github, &["ls-tree", "-r", "--name-only", "newgit"]);
    assert!(files.lines().any(|line| line == "src/new.ts"), "{files}");
    assert!(!files.contains("apps/"), "{files}");
}

#[test]
fn a_root_that_mounts_a_tracker_in_git_ships_what_inherits_its_export() {
    let (_guard, temp) = tempdir();
    let upstream = upstream(&temp);
    let store = project(&temp);
    let repo = store.paths().project_root.clone();
    let github = github_copy(&temp, &upstream, "t3code");
    import(&store, &upstream);
    manager(&store).merge("main", "main").expect("publish");
    // t3code mounts `ui` of its own, defined in its own .newgit/trackers,
    // which ships wherever t3code does.
    commit_as(
        &repo,
        "ana",
        1_700_000_000,
        &[
            (
                "apps/t3code/.newgit/trackers/ui.toml",
                "audience = \"project-devs\"\nstorage = \"local\"\ngit = true\nmount = \"ui\"\npaths = [\".\"]\nexport = \"inherit\"\n",
            ),
            ("apps/t3code/ui/button.ts", "b\n"),
        ],
        "t3code: mount ui",
    );
    manager(&store).merge("main", "main").expect("merge");
    rooted(&store, &github);
    manager(&store)
        .export_to_remote(TARGET, RemoteExportOptions::default())
        .expect("export");
    let files = git(&github, &["ls-tree", "-r", "--name-only", "newgit"]);
    assert!(files.lines().any(|line| line == "ui/button.ts"), "{files}");
    assert!(!files.contains(".newgit/pins"), "{files}");
}
