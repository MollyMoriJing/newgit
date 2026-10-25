//! `newgit spawn --from-checkpoint --json` had the gap `spawn_json_failure.rs`
//! covers for a plain spawn: it exited 0 whatever came up, and named only
//! what the restore did. It reports each resource as `spawn --json` does,
//! and exits 1 the same way, when the instance it leaves is not ready.
//!
//! New tests live in their own file per AGENTS.md's "Working in parallel"
//! conventions, rather than appended to an existing integration test file.

#![allow(dead_code)]

include!("support/fake_remote.rs");

/// `feature-a`, spawned and checkpointed as `feature-a/ckpt_001`.
fn checkpointed(fixture: &Fixture) {
    fixture.ok(&fixture.store, &["spawn", "feature-a"]);
    let workspace = workspace_of(fixture, "feature-a");
    fixture.ok(&workspace, &["checkpoint"]);
}

struct Spawned {
    status: std::process::ExitStatus,
    json: Value,
    stdout: String,
}

impl Spawned {
    /// The `resources` entry naming `name`.
    fn resource(&self, name: &str) -> &Value {
        self.json["resources"]
            .as_array()
            .and_then(|resources| resources.iter().find(|resource| resource["name"] == name))
            .unwrap_or_else(|| panic!("no `resources` entry for `{name}`:\n{}", self.stdout))
    }
}

/// `review`, spawned from `feature-a/ckpt_001` with `--json`, its stdout
/// parsed as the one JSON document it must be.
fn spawn_review(fixture: &Fixture) -> Spawned {
    let output = fixture.newgit(
        &fixture.store,
        &[
            "spawn",
            "review",
            "--from-checkpoint",
            "feature-a/ckpt_001",
            "--json",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let json = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not one JSON document ({error}):\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    Spawned {
        status: output.status,
        json,
        stdout,
    }
}

#[test]
fn a_restore_that_fails_exits_nonzero_and_says_which() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    std::fs::write(
        fixture.store.join(".newgit/resources/seed.toml"),
        r#"ownership = "branch"

[restore]
mode = "command"
command = "exit 1"
"#,
    )
    .expect("resource");
    checkpointed(&fixture);

    let spawned = spawn_review(&fixture);
    assert_eq!(spawned.status.code(), Some(1), "{}", spawned.stdout);
    let seed = spawned.resource("seed");
    assert_eq!(seed["status"], "failed", "{}", spawned.stdout);
    assert_eq!(seed["action"], "command", "{}", spawned.stdout);
    assert_eq!(seed["ok"], false, "{}", spawned.stdout);
    let recovery = spawned.json["recovery_record"]
        .as_str()
        .unwrap_or_else(|| panic!("a failed restore writes one:\n{}", spawned.stdout));
    assert!(Utf8Path::new(recovery).is_file(), "{recovery}");

    // Exit 1 reports the state; the instance is kept, as a plain spawn's is.
    assert!(workspace_of(&fixture, "review").is_dir());
}

/// The restore completing is not the whole answer: a resource the checkpoint
/// holds no state for keeps whatever the spawn's own bind left it in.
#[test]
fn a_resource_the_checkpoint_never_had_counts_too() {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    checkpointed(&fixture);
    std::fs::write(
        fixture.store.join(".newgit/resources/late.toml"),
        r#"ownership = "branch"

[actions.prepare]
command = "exit 1"
"#,
    )
    .expect("resource");

    let spawned = spawn_review(&fixture);
    assert_eq!(spawned.status.code(), Some(1), "{}", spawned.stdout);
    let late = spawned.resource("late");
    assert_eq!(late["status"], "failed", "{}", spawned.stdout);
    assert!(late["prepare_log"].is_string(), "{}", spawned.stdout);
    assert_eq!(
        late["ok"],
        Value::Null,
        "nothing was restored for it:\n{}",
        spawned.stdout
    );
    assert_eq!(spawned.json["recovery_record"], Value::Null);
}
