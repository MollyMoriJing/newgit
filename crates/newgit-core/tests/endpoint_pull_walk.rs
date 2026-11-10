//! A look says every commit a pull would bring in beyond `main`, not only the
//! pulled one (#399). A merged pull request refused for an instance's withheld
//! work stays on the mirror, and the next commit there carries it: the Worker
//! asks GitHub about the whole walk, so the later pull is refused the same
//! way, until `newgit merge <instance>` lands the work and it goes through.

mod imports;
mod mirror_remote;

use std::collections::BTreeMap;

use imports::*;
use mirror_remote::*;
use newgit_core::endpoint::{MirrorInstance, PullAnswer, WalkedCommit, pull_mirror};

fn answer(
    remote: &Remote,
    list: &BTreeMap<String, String>,
    commit: &str,
    instances: Vec<MirrorInstance>,
    peek: bool,
) -> PullAnswer {
    let mut plan = remote.plan(list, commit);
    plan.instances = instances;
    plan.peek = peek;
    let answer = pull_mirror(&plan).expect("pull");
    remote.follow(&answer);
    answer
}

fn walk(remote: &Remote, list: &BTreeMap<String, String>, commit: &str) -> Vec<WalkedCommit> {
    match answer(remote, list, commit, Vec::new(), true) {
        PullAnswer::Work { commits } => commits,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_look_names_every_commit_beyond_main_with_its_subject_and_parents() {
    let (_guard, temp) = tempdir();
    let remote = remote(&temp);
    let base = remote.index();
    let (merged, _) = merged_pr(&remote);
    let later = remote.commit_on_mirror("later", &[("later.txt", "later\n")], "a later fix");

    let commits = walk(&remote, &base, &later);
    let last = commits.last().expect("a walk");
    assert_eq!(last.commit, later, "oldest first, the pulled commit last");
    assert_eq!(last.subject, "a later fix");
    assert_eq!(last.parents, vec![merged.clone()]);
    let merge = commits
        .iter()
        .find(|walked| walked.commit == merged)
        .expect("the earlier merge is in the walk");
    assert_eq!(merge.subject, "Merge pull request #7 from acme/feature");
    assert_eq!(merge.parents.len(), 2, "{merge:?}");
    assert!(
        commits
            .iter()
            .any(|walked| walked.commit == merge.parents[1]),
        "the branch it merged is in the walk: {commits:?}"
    );
    assert!(
        commits
            .iter()
            .all(|walked| walked.commit != merge.parents[0]),
        "`main` is not"
    );
}

#[test]
fn a_later_commit_carrying_a_refused_merge_is_refused_until_newgit_merge() {
    let (_guard, temp) = tempdir();
    let remote = remote(&temp);
    let (_, instance) = merged_pr(&remote);
    let later = remote.commit_on_mirror("later", &[("later.txt", "later\n")], "a later fix");
    let base = remote.index();
    let root = remote.main("root");

    // The Worker names `feature` from the merge in the walk, so the later
    // commit is refused as the merge was, and nothing moves.
    match answer(&remote, &base, &later, vec![instance.clone()], false) {
        PullAnswer::Refused { why } => {
            assert!(why.contains(&format!("commit {later}")), "{why}");
            assert!(why.contains("brings in `feature`'s branch"), "{why}");
            assert!(why.contains("`newgit merge feature`"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(remote.main("root"), root);

    // Not refused forever: once the work is on `main`, the same instance,
    // named again, has nothing to lose, and both commits come in.
    manager(&remote.store)
        .merge("feature", "main")
        .expect("newgit merge feature");
    remote.publish();
    let merged = remote.index();
    let list = match answer(&remote, &merged, &later, vec![instance], false) {
        PullAnswer::Landed { list, .. } => list,
        other => panic!("{other:?}"),
    };
    assert_eq!(list["t3code"], merged["t3code"], "the t3code edit is kept");
    assert_eq!(git(&remote.repos["t3code"], &["show", "main:x.ts"]), "x");
    let root = &remote.repos["root"];
    assert_eq!(git(root, &["show", "main:src/y.rs"]), "y");
    assert_eq!(git(root, &["show", "main:later.txt"]), "later");
}
