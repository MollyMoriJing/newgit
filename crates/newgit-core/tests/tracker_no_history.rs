//! A definition has no `history`: every tracker rolls up on the remote one
//! commit per change, so the key is refused like any other unknown one.
//!
//! New tests live in their own file per AGENTS.md's "Working in parallel".

use camino::Utf8Path;
use newgit_core::tracker::TrackerDefinition;

const BASE: &str = "audience = \"project-devs\"\nstorage = \"local\"\ngit = true\npaths = [\".\"]\nmount = \"users\"\n";

fn parse(text: &str) -> newgit_core::error::Result<TrackerDefinition> {
    TrackerDefinition::from_text("users", Utf8Path::new("users.toml"), text)
}

#[test]
fn a_definition_that_says_history_is_refused() {
    parse(BASE).expect("parses without it");
    for value in ["commits", "pins"] {
        let error = parse(&format!("{BASE}history = \"{value}\"\n")).expect_err("refused");
        let cause = std::error::Error::source(&error).map(ToString::to_string);
        assert!(
            cause.is_some_and(|cause| cause.contains("unknown field `history`")),
            "refused as an unknown key: {error}"
        );
    }
}
