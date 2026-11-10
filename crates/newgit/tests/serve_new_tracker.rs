// A tracker in Git carved in the project root after it was linked reaches the
// remote with `newgit merge main`. Its repo is seeded with its commit, and the
// merge must not also upload that commit: the remote reads each tracker's
// definition from `access`'s `main`, which has no definition for the new one
// until this very merge lands, so it refuses the upload. Found carving `serve`
// and `cloudflare` out of newgit's own root. newgit.dev answered 404 for both
// uploads, and the fake remote never checked.

include!("support/serve.rs");

#[test]
fn a_tracker_carved_after_linking_lands_with_merge_main() {
    let (_guard, temp) = tempdir();
    let server = start(&temp);
    let home = temp.join("jack");
    let token = login(&server, &home, "jack");

    let store = temp.join("store");
    std::fs::create_dir_all(store.join("crates/lib")).expect("mkdir");
    git(&home, &store, &["init", "-q", "-b", "main"]);
    std::fs::write(store.join("README.md"), "root\n").expect("write");
    std::fs::write(store.join("crates/lib/lib.rs"), "pub fn lib() {}\n").expect("write");
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
    newgit(&home, &store, &["remote", "add", "--url", &server.url]);
    // `remote add` wrote `[remote]` to the config: committed, as it says to.
    git(&home, &store, &["commit", "-q", "-am", "link"]);

    newgit(
        &home,
        &store,
        &["tracker", "create", "lib", "--git", "--path", "crates/lib"],
    );
    newgit(&home, &store, &["merge", "main"]);

    // The remote has the tracker on `main`, and plain Git sees its files.
    let (status, project) = http(
        &server.url,
        "GET",
        "/v1/projects/jack/store/branches/main",
        &[("authorization", &format!("Bearer {token}"))],
        "",
    );
    assert_eq!(status, 200, "{project}");
    assert!(project["list"]["lib"].is_string(), "{project}");
    let clone = temp.join("clone");
    let cloned = git_as(
        &token,
        &temp,
        &[
            "clone",
            "-q",
            &format!("{}/jack/store.git", server.url),
            clone.as_str(),
        ],
    );
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(clone.join("crates/lib/lib.rs")).expect("lib.rs"),
        "pub fn lib() {}\n"
    );
}
