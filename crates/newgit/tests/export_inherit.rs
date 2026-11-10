//! `export` in a tracker's definition (#201): a tracker declaring a URL is a
//! target's root, `"inherit"` ships wherever the parent ships, and no
//! `export` ships nowhere. Carving writes `"inherit"`, so a directory carved
//! out of a mirrored tracker stays in the mirror. A bare repository stands in
//! for each GitHub repo.
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

/// Root's mirror `github`, which `access` inherits, committed and published.
fn mirrored(fixture: &Fixture) -> Utf8PathBuf {
    let mirror = bare(fixture, "github");
    set_export(
        &fixture.store,
        "root",
        Some(&export_to(&mirror, "github", "")),
    );
    set_export(&fixture.store, "access", Some("\"inherit\""));
    commit_all(fixture, "export");
    mirror
}

fn commit_all(fixture: &Fixture, message: &str) {
    git(&fixture.store, &["add", "-A"]);
    git(&fixture.store, &["commit", "-q", "-m", message]);
    fixture.ok(&fixture.store, &["merge", "main"]);
}

/// `dir/lib.rs` committed, then carved into tracker `name` with `flags`.
fn carve(fixture: &Fixture, name: &str, dir: &str, flags: &[&str]) -> String {
    std::fs::create_dir_all(fixture.store.join(dir)).expect("mkdir");
    std::fs::write(fixture.store.join(dir).join("lib.rs"), "pub fn f() {}\n").expect("write");
    git(&fixture.store, &["add", "-A"]);
    git(
        &fixture.store,
        &["commit", "-q", "-m", &format!("add {dir}")],
    );
    let mut args = vec!["tracker", "create", name, "--git", "--path", dir];
    args.extend_from_slice(flags);
    let said = fixture.ok(&fixture.store, &args);
    fixture.ok(&fixture.store, &["merge", "main"]);
    said
}

fn definition(fixture: &Fixture, name: &str) -> String {
    std::fs::read_to_string(fixture.store.join(format!(".newgit/trackers/{name}.toml")))
        .expect("definition")
}

fn refused(fixture: &Fixture, args: &[&str]) -> String {
    let output = fixture.newgit(&fixture.store, args);
    assert!(
        !output.status.success(),
        "newgit {args:?} should have failed"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_carved_directory_inherits_the_export_and_stays_in_the_mirror() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let mirror = mirrored(&fixture);

    let said = carve(&fixture, "serve", "crates/serve", &[]);
    assert!(said.contains("inherits `root`'s"), "{said}");
    assert!(
        definition(&fixture, "serve").contains("export = \"inherit\""),
        "{}",
        definition(&fixture, "serve")
    );

    let exported = fixture.ok(&fixture.store, &["export", "--remote", "github"]);
    assert!(exported.contains("serve"), "{exported}");
    assert!(!exported.contains("withheld: serve"), "{exported}");
    let files = files_at(&mirror, "main");
    assert!(files.contains("crates/serve/lib.rs"), "{files}");
}

#[test]
fn a_narrower_audience_is_carved_with_no_export_and_cannot_inherit() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let mirror = mirrored(&fixture);

    let said = carve(&fixture, "secret", "secret", &["--audience", "user:jack"]);
    assert!(said.contains("export:    none"), "{said}");
    assert!(
        !definition(&fixture, "secret").contains("export"),
        "{}",
        definition(&fixture, "secret")
    );
    let exported = fixture.ok(&fixture.store, &["export", "--remote", "github"]);
    assert!(exported.contains("withheld: secret"), "{exported}");
    assert!(!files_at(&mirror, "main").contains("secret/"));

    // Written by hand, `"inherit"` would send it past its audience.
    set_export(&fixture.store, "secret", Some("\"inherit\""));
    let stderr = refused(&fixture, &["export", "--remote", "github", "--dry-run"]);
    assert!(stderr.contains("past its audience"), "{stderr}");
    assert!(stderr.contains("secret"), "{stderr}");
}

#[test]
fn a_tracker_ships_inside_the_project_and_on_its_own() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let mirror = mirrored(&fixture);
    carve(&fixture, "lib", "libs/lib", &[]);
    let own = bare(&fixture, "lib");
    set_export(
        &fixture.store,
        "lib",
        Some(&format!("[\"inherit\", {}]", export_to(&own, "lib", ""))),
    );
    commit_all(&fixture, "lib ships on its own too");

    fixture.ok(&fixture.store, &["export", "--remote", "github"]);
    fixture.ok(&fixture.store, &["export", "--remote", "lib"]);
    assert!(files_at(&mirror, "main").contains("libs/lib/lib.rs"));
    let alone = files_at(&own, "main");
    assert_eq!(alone, "lib.rs", "rooted at it, its files at the top");
}

#[test]
fn no_export_never_ships() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let mirror = mirrored(&fixture);
    carve(&fixture, "site", "site", &[]);
    set_export(&fixture.store, "site", None);
    commit_all(&fixture, "site stays here");

    let exported = fixture.ok(&fixture.store, &["export", "--remote", "github"]);
    assert!(exported.contains("withheld: site"), "{exported}");
    assert!(!files_at(&mirror, "main").contains("site/"));
}

#[test]
fn two_trackers_cannot_declare_one_repository() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    let mirror = mirrored(&fixture);
    carve(&fixture, "lib", "libs/lib", &[]);
    set_export(
        &fixture.store,
        "lib",
        Some(&export_to(&mirror, "again", "")),
    );
    let stderr = refused(&fixture, &["export", "--remote", "github"]);
    assert!(stderr.contains("\"inherit\""), "{stderr}");
}
