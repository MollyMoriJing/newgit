//! A project's tags go only to export targets carrying root's history: the
//! one root declares, which every inheriting tracker ships inside. A target
//! declared by a child tracker is that tracker's own history, with versions
//! of its own (t3code's `v0.1.x`), and a tag never moves, so the project's
//! tag there would take the name from it for good. Tags coming in from such
//! a target are refused for the same reason.
//!
//! New tests live in their own file per AGENTS.md's "Working in parallel".

#![allow(dead_code)]

include!("support/fake_remote.rs");

fn bare(fixture: &Fixture, name: &str) -> Utf8PathBuf {
    let repo = fixture.temp.join(format!("{name}.git"));
    git(
        &fixture.temp,
        &["init", "-q", "--bare", "-b", "main", repo.as_str()],
    );
    repo
}

/// `git push` as someone with newgit installed runs it, from the project root.
fn push(fixture: &Fixture, args: &[&str]) -> String {
    let binary = Utf8PathBuf::from(env!("CARGO_BIN_EXE_newgit"));
    let output = Command::new("git")
        .arg("-C")
        .arg(fixture.store.as_str())
        .arg("push")
        .args(args)
        .env("HOME", fixture.temp.join("home").as_str())
        .env(
            "PATH",
            format!(
                "{}:{}",
                binary.parent().expect("a directory"),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");
    stderr
}

#[test]
fn a_tag_skips_targets_rooted_at_a_child_and_leaves_their_own_versions_alone() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let store = fixture.store.clone();

    // t3code, imported, with a `v1.0.0` of its own where it came from.
    let upstream = temp.join("upstream");
    std::fs::create_dir_all(&upstream).expect("mkdir");
    git(&upstream, &["init", "-q", "-b", "newgit"]);
    std::fs::write(upstream.join("app.ts"), "app\n").expect("write");
    git(&upstream, &["add", "-A"]);
    git(&upstream, &["commit", "-q", "-m", "t3: first"]);
    git(&upstream, &["tag", "v1.0.0"]);
    let t3code = temp.join("t3code.git");
    git(
        &temp,
        &["clone", "-q", "--bare", upstream.as_str(), t3code.as_str()],
    );
    let theirs = git(&t3code, &["rev-parse", "refs/tags/v1.0.0"]);
    fixture.ok(
        &store,
        &[
            "tracker",
            "create",
            "t3code",
            "--git",
            "--mount",
            "apps/t3code",
            "--from",
            upstream.as_str(),
        ],
    );

    // A library carved out of root that ships inside root's mirror and on
    // its own: only the first carries root's history.
    std::fs::create_dir_all(store.join("libs/lib")).expect("mkdir");
    std::fs::write(store.join("libs/lib/lib.rs"), "pub fn f() {}\n").expect("write");
    git(&store, &["add", "-A"]);
    git(&store, &["commit", "-q", "-m", "add lib"]);
    fixture.ok(
        &store,
        &["tracker", "create", "lib", "--git", "--path", "libs/lib"],
    );

    let mirror = bare(&fixture, "mirror");
    let lib = bare(&fixture, "lib");
    set_export(&store, "root", Some(&export_to(&mirror, "github", "")));
    set_export(&store, "access", Some("\"inherit\""));
    set_export(
        &store,
        "t3code",
        Some(&export_to(&t3code, "t3code", "branch = \"newgit\"")),
    );
    set_export(
        &store,
        "lib",
        Some(&format!("[\"inherit\", {}]", export_to(&lib, "lib", ""))),
    );
    git(&store, &["add", "-A"]);
    git(&store, &["commit", "-q", "-m", "targets"]);
    fixture.ok(&store, &["merge", "main"]);
    for target in ["github", "t3code", "lib"] {
        fixture.ok(&store, &["export", "--remote", target]);
    }

    std::fs::write(store.join("ours.txt"), "ours\n").expect("write");
    git(&store, &["add", "-A"]);
    git(&store, &["commit", "-q", "-m", "release work"]);
    git(&store, &["tag", "v1.0.0"]);
    let said = push(&fixture, &["origin", "main", "v1.0.0"]);

    assert!(said.contains("tagged `v1.0.0` on `github`"), "{said}");
    assert_eq!(
        git(&mirror, &["rev-parse", "refs/tags/v1.0.0"]),
        git(&mirror, &["rev-parse", "main"])
    );
    assert!(!said.contains("on `t3code`"), "{said}");
    assert!(!said.contains("on `lib`"), "{said}");
    assert_eq!(
        git(&t3code, &["rev-parse", "refs/tags/v1.0.0"]),
        theirs,
        "t3code's own v1.0.0 is where it was"
    );
    assert_eq!(git(&lib, &["tag", "--list"]), "");
}
