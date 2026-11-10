//! `newgit status` says what a webhook pull is waiting on, and every pull
//! refused or given up since its target last landed (#392, #353): a pull
//! waiting for GitHub to say whose it is, one waiting out a failure, a queue
//! nothing was moving, and the refusals of 2026-11-09, which a later landing
//! or an event finding its commit on `main` ends.
//!
//! New tests live in their own file per AGENTS.md's "Working in parallel".

#![allow(dead_code)]

include!("support/fake_remote.rs");

fn status_with(remote_state: serde_json::Value) -> String {
    let (_guard, temp) = tempdir();
    let fixture = setup(&temp);
    fixture.ok(&fixture.store, &["spawn", "feature-a"]);
    fixture.remote.state.lock().expect("state").remote_state = remote_state;
    fixture.ok(&fixture.store, &["status"])
}

#[test]
fn status_says_what_a_waiting_pull_waits_on() {
    let status = status_with(json!({
        "pulls": [
            {
                "target": "github", "commit": "a".repeat(40), "queuedAt": "2026-11-09T21:11:55Z",
                "attempts": 0, "step": "asking GitHub",
                "lookupFailed": "dial tcp 140.82.116.6:443: i/o timeout"
            },
            {
                "target": "github", "commit": "b".repeat(40), "queuedAt": "2026-11-09T21:17:05Z",
                "attempts": 0, "step": "queued"
            }
        ]
    }));
    assert!(
        status.contains(
            "the webhook's pull from `github` at aaaaaaaaaaaa is waiting (asking GitHub which pull \
             request it came from; GitHub could not say (dial tcp 140.82.116.6:443: i/o timeout), \
             so it waits and asks again)"
        ),
        "{status}"
    );
    assert!(
        !status.contains("0 failed attempts"),
        "waiting on GitHub is not a failed attempt: {status}"
    );
    assert!(
        status.contains(
            "the webhook's pull from `github` at bbbbbbbbbbbb is waiting (behind another)"
        ),
        "{status}"
    );
}

#[test]
fn status_says_a_failing_pull_and_when_it_is_tried_again() {
    let status = status_with(json!({
        "pulls": [{
            "target": "github", "commit": "a".repeat(40), "queuedAt": "2026-11-09T21:11:47Z",
            "attempts": 1, "step": "landing",
            "lastError": "the Git endpoint could not pull: newgit endpoint-pull failed: Error: Network connection lost.",
            "retryAt": "2026-11-09T21:12:05Z"
        }]
    }));
    assert!(
        status.contains(
            "is waiting (landing; 1 failed attempt; last: the Git endpoint could not pull: newgit \
             endpoint-pull failed: Error: Network connection lost.; tried again at 2026-11-09T21:12:05Z)"
        ),
        "{status}"
    );
}

#[test]
fn status_says_a_queue_nothing_was_moving() {
    let status = status_with(json!({
        "pulls": [{
            "target": "github", "commit": "a".repeat(40), "queuedAt": "2026-11-06T00:00:00Z",
            "attempts": 0, "step": "looking", "stalled": true
        }]
    }));
    assert!(
        status.contains(
            "(looking at the mirror; nothing was moving it on; asking for this started it again)"
        ),
        "{status}"
    );
}

#[test]
fn status_says_every_refusal_since_the_last_landing_until_one_lands() {
    let refused = |commit: &str, at: &str| {
        json!({ "target": "github", "commit": commit.repeat(40), "outcome": "refused",
                "why": "came from `feature`'s branch", "at": at })
    };
    let ended = vec![
        json!({ "target": "github", "commit": "1".repeat(40), "outcome": "landed", "at": "2026-11-09T20:00:00Z" }),
        json!({ "target": "github", "commit": "9".repeat(40), "outcome": "gave up",
                "why": "it waited 2 hours", "at": "2026-11-09T21:42:59Z" }),
        refused("7", "2026-11-09T21:43:09Z"),
        refused("8", "2026-11-09T21:43:24Z"),
        // A redelivery refused again is said once.
        refused("8", "2026-11-09T21:50:00Z"),
    ];
    let status = status_with(json!({ "ended": ended }));
    assert!(
        status.contains("the webhook's pull from `github` at 999999999999 gave up (2026-11-09T21:42:59Z): it waited 2 hours"),
        "{status}"
    );
    assert!(status.contains("at 777777777777 was refused"), "{status}");
    assert_eq!(
        status.matches("at 888888888888 was refused").count(),
        1,
        "{status}"
    );
    assert!(
        status.contains("(2026-11-09T21:50:00Z)"),
        "the latest is said: {status}"
    );

    // An event that found its commit on `main` already ends them, as a
    // landing does.
    let mut over = ended.clone();
    over.push(json!({ "target": "github", "commit": "c".repeat(40), "outcome": "nothing", "at": "2026-11-09T22:00:00Z" }));
    let status = status_with(json!({ "ended": over }));
    assert!(!status.contains("was refused"), "{status}");
    assert!(!status.contains("gave up"), "{status}");
}
