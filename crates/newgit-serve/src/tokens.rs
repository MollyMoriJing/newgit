//! newgit tokens: `ngt_<payload>.<signature>`, HMAC-SHA256 over the payload,
//! the same format the Worker issues, so the CLI reads them unchanged.
//!
//! A token is also the Git password for every repo on this remote: there are
//! no per-repo passwords to mint, store, or revoke. Each Git request is
//! checked against the rules when it arrives, so revoking a token, or taking
//! someone out of a project, takes effect on their next request.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit as _, Mac as _};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const PREFIX: &str = "ngt_";
const DAY: i64 = 60 * 60 * 24;

/// A human's login lasts this long.
pub const LOGIN_TTL_SECONDS: i64 = 30 * DAY;
/// An agent's token defaults to this, and may not exceed the cap.
pub const AGENT_TTL_SECONDS: i64 = 7 * DAY;
pub const AGENT_TTL_MAX_SECONDS: i64 = 30 * DAY;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Token id: what revocation names.
    pub jti: String,
    /// The GitHub login it acts as, lowercased. Agents act as their owner.
    pub sub: String,
    /// `login` or `agent`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The one project it may touch, when pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The one instance it may write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    pub iat: i64,
    pub exp: i64,
}

pub struct NewToken<'a> {
    pub kind: &'a str,
    pub login: &'a str,
    pub agent: Option<&'a str>,
    pub project: Option<&'a str>,
    pub instance: Option<&'a str>,
    pub ttl_seconds: i64,
}

fn mac(secret: &[u8]) -> Hmac<Sha256> {
    // HMAC takes a key of any length.
    Hmac::<Sha256>::new_from_slice(secret).unwrap_or_else(|_| unreachable!())
}

pub fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).unwrap_or_else(|error| panic!("no randomness: {error}"));
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sign(secret: &[u8], spec: &NewToken<'_>, now: i64) -> (String, Claims) {
    let claims = Claims {
        jti: format!("tok_{}", random_hex(10)),
        sub: spec.login.to_lowercase(),
        kind: spec.kind.to_owned(),
        agent: spec.agent.map(str::to_owned),
        project: spec.project.map(str::to_owned),
        instance: spec.instance.map(str::to_owned),
        iat: now,
        exp: now + spec.ttl_seconds,
    };
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap_or_default());
    let mut signer = mac(secret);
    signer.update(body.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(signer.finalize().into_bytes());
    (format!("{PREFIX}{body}.{signature}"), claims)
}

/// The claims of a correctly signed, unexpired token; `None` for anything
/// else. Says nothing about revocation: that is the tokens table.
pub fn verify(secret: &[u8], token: &str, now: i64) -> Option<Claims> {
    let rest = token.strip_prefix(PREFIX)?;
    let mut parts = rest.split('.');
    let (body, signature) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
    let mut verifier = mac(secret);
    verifier.update(body.as_bytes());
    verifier.verify_slice(&signature).ok()?;
    let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
    if claims.kind != "login" && claims.kind != "agent" {
        return None;
    }
    (claims.exp > now).then_some(claims)
}

/// Whether `list` names `login`: logins separated by commas or whitespace,
/// or `*` for anyone. An empty list names no one.
pub fn listed(list: &str, login: &str) -> bool {
    list.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| entry == "*" || entry.eq_ignore_ascii_case(login))
}

/// `a == b` in time that does not depend on where they differ.
pub fn same_secret(given: &str, secret: &str) -> bool {
    use sha2::Digest as _;
    let (a, b) = (Sha256::digest(given), Sha256::digest(secret));
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let spec = NewToken {
            kind: "login",
            login: "Jack",
            agent: None,
            project: None,
            instance: None,
            ttl_seconds: 60,
        };
        let (token, claims) = sign(b"key", &spec, 1000);
        assert_eq!(verify(b"key", &token, 1000), Some(claims));
        assert_eq!(verify(b"other", &token, 1000), None);
        assert_eq!(verify(b"key", &token, 2000), None);
        assert!(listed("ana, jack", "JACK"));
        assert!(listed("*", "x"));
        assert!(!listed("", "x"));
    }
}
