// A view rooted at a child tracker (`/<owner>/<name>/<child>.git`) composes
// the same commits whatever was composed before it, and expands its own
// children at every commit of its history, not only at the tip.
//
// Found on a copy of newgit itself: `t3code`'s view, composed in fresh
// stores, lacked `.repos` at the commit that created it, and was cached that
// way; composed after root's view it was right. Snapshotting the history's
// pins named them from `root` instead of the view's root, so `t3code`'s
// pin `repos` was looked for as `repos`, not `t3code__repos`.

include!("support/serve.rs");

/// A project whose `lib` tracker has a child, `vendor`, that moved twice
/// after it was carved, so `lib`'s history pins three different commits of it.
fn nested_project(temp: &Utf8Path, server: &Server, home: &Utf8Path) -> Utf8PathBuf {
    let store = temp.join("store");
    std::fs::create_dir_all(store.join("apps/lib/vendor")).expect("mkdir");
    git(home, &store, &["init", "-q", "-b", "main"]);
    std::fs::write(store.join("README.md"), "root\n").expect("write");
    std::fs::write(store.join("apps/lib/a.txt"), "a\n").expect("write");
    std::fs::write(store.join("apps/lib/vendor/v.txt"), "v1\n").expect("write");
    git(home, &store, &["add", "."]);
    git(home, &store, &["commit", "-q", "-m", "initial"]);
    newgit(home, &store, &["init"]);
    let config = store.join(".newgit/config.toml");
    let mut text = std::fs::read_to_string(&config).expect("config");
    text.push_str(&format!(
        "\n[workspace]\nroot = \"{}\"\n",
        temp.join("workspaces")
    ));
    std::fs::write(&config, text).expect("write config");
    git(home, &store, &["add", "-A"]);
    git(home, &store, &["commit", "-q", "-m", "newgit"]);
    newgit(
        home,
        &store,
        &["tracker", "create", "lib", "--git", "--path", "apps/lib"],
    );
    newgit(
        home,
        &store,
        &[
            "tracker",
            "create",
            "vendor",
            "--git",
            "--path",
            "apps/lib/vendor",
        ],
    );
    for version in ["v2", "v3"] {
        std::fs::write(store.join("apps/lib/vendor/v.txt"), format!("{version}\n")).expect("write");
        git(home, &store, &["commit", "-q", "-am", version]);
    }
    newgit(home, &store, &["remote", "add", "--url", &server.url]);
    store
}

fn clone(token: &str, temp: &Utf8Path, url: &str, dir: &str) -> Utf8PathBuf {
    let into = temp.join(dir);
    let cloned = git_as(token, temp, &["clone", "-q", url, into.as_str()]);
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    into
}

/// The child's content at the commit of `lib`'s view whose subject is `subject`.
fn vendor_at(view: &Utf8Path, subject: &str) -> String {
    let commit = git(
        view,
        view,
        &["log", "--format=%H", &format!("--grep=^{subject}$")],
    );
    let shown = run(
        "git",
        view,
        view,
        &["show", &format!("{commit}:vendor/v.txt")],
    );
    String::from_utf8_lossy(&shown.stdout).trim().to_owned()
}

#[test]
fn a_rooted_view_expands_its_children_at_every_commit_in_fresh_stores() {
    let (_guard, temp) = tempdir();
    let server = start(&temp);
    let home = temp.join("jack");
    let token = login(&server, &home, "jack");
    nested_project(&temp, &server, &home);

    let view = clone(
        &token,
        &temp,
        &format!("{}/jack/store/lib.git", server.url),
        "lib",
    );
    assert_eq!(
        vendor_at(&view, "v2"),
        "v2",
        "a historical pin was left unexpanded"
    );
    assert_eq!(vendor_at(&view, "v3"), "v3");
}

#[test]
fn a_rooted_view_is_the_same_whatever_was_composed_before_it() {
    let (_guard, temp) = tempdir();
    let first = start(&temp.join("first"));
    let second = start(&temp.join("second"));
    let home = temp.join("jack");
    let tokens = [login(&first, &home, "jack"), login(&second, &home, "jack")];
    // One project on two remotes: the same trackers, so the same views.
    let store = nested_project(&temp, &first, &home);
    let config = store.join(".newgit/config.toml");
    let text = std::fs::read_to_string(&config).expect("config");
    let unlinked = text[..text.find("[remote]").expect("linked")].to_owned();
    std::fs::write(&config, unlinked).expect("unlink");
    git(&home, &store, &["commit", "-q", "-am", "unlink"]);
    newgit(&home, &store, &["remote", "add", "--url", &second.url]);

    // On the first, the rooted view first; on the second, root's view first.
    let alone = clone(
        &tokens[0],
        &temp,
        &format!("{}/jack/store/lib.git", first.url),
        "alone",
    );
    clone(
        &tokens[1],
        &temp,
        &format!("{}/jack/store.git", second.url),
        "root",
    );
    let after = clone(
        &tokens[1],
        &temp,
        &format!("{}/jack/store/lib.git", second.url),
        "after",
    );
    assert_eq!(
        git(&home, &alone, &["rev-parse", "HEAD"]),
        git(&home, &after, &["rev-parse", "HEAD"]),
        "the same view composed to different commits"
    );
}

#[test]
fn a_rooted_view_after_roots_reads_the_whole_history_not_roots_snapshots() {
    // Root pins only the tip of a tracker imported with its own history, so
    // composing root's view leaves that tip in the snapshot store one commit
    // deep. The rooted view after it must still fetch the whole history
    // rather than walk from that snapshot, which ends at the cut.
    let (_guard, temp) = tempdir();
    let server = start(&temp);
    let home = temp.join("jack");
    let token = login(&server, &home, "jack");
    let upstream = temp.join("upstream");
    std::fs::create_dir_all(&upstream).expect("mkdir");
    git(&home, &upstream, &["init", "-q", "-b", "main"]);
    for version in ["one", "two", "three"] {
        std::fs::write(upstream.join("lib.txt"), format!("{version}\n")).expect("write");
        git(&home, &upstream, &["add", "."]);
        git(&home, &upstream, &["commit", "-q", "-m", version]);
    }
    let store = temp.join("store");
    std::fs::create_dir_all(&store).expect("mkdir");
    git(&home, &store, &["init", "-q", "-b", "main"]);
    std::fs::write(store.join("README.md"), "root\n").expect("write");
    git(&home, &store, &["add", "."]);
    git(&home, &store, &["commit", "-q", "-m", "initial"]);
    newgit(&home, &store, &["init"]);
    let config = store.join(".newgit/config.toml");
    let mut text = std::fs::read_to_string(&config).expect("config");
    text.push_str(&format!(
        "\n[workspace]\nroot = \"{}\"\n",
        temp.join("workspaces")
    ));
    std::fs::write(&config, text).expect("write config");
    git(&home, &store, &["add", "-A"]);
    git(&home, &store, &["commit", "-q", "-m", "newgit"]);
    newgit(
        &home,
        &store,
        &[
            "tracker",
            "create",
            "lib",
            "--git",
            "--from",
            upstream.as_str(),
            "--mount",
            "apps/lib",
        ],
    );
    newgit(&home, &store, &["remote", "add", "--url", &server.url]);

    clone(
        &token,
        &temp,
        &format!("{}/jack/store.git", server.url),
        "root",
    );
    let view = clone(
        &token,
        &temp,
        &format!("{}/jack/store/lib.git", server.url),
        "lib",
    );
    assert_eq!(
        git(&home, &view, &["rev-parse", "HEAD"]),
        git(&home, &upstream, &["rev-parse", "HEAD"])
    );
    assert_eq!(git(&home, &view, &["rev-list", "--count", "HEAD"]), "3");
}
