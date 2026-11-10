//! Bare repositories on local disk, worked with real Git.
//!
//! Everything here is a short, local Git command. What the Worker read from
//! Artifacts over its binding (a file at a branch, a branch's head, a commit's
//! pins) is `git cat-file` here.

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

use camino::Utf8Path;

use crate::error::{ApiResult, HttpError};
use crate::names::namespace_prefix;

/// Config every repo gets: check what is pushed, never prune what a fork's
/// namespace or a landing might still name, and let `newgit clone` filter.
const REPO_CONFIG: &[(&str, &str)] = &[
    ("receive.fsckObjects", "true"),
    ("uploadpack.allowFilter", "true"),
    ("core.logAllRefUpdates", "always"),
];

pub fn git(dir: &Utf8Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git")
        .arg("--git-dir")
        .arg(dir.as_str())
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
}

fn checked(dir: &Utf8Path, args: &[&str]) -> ApiResult<String> {
    let output = git(dir, args)?;
    if !output.status.success() {
        return Err(HttpError::internal(format!(
            "git {args:?} in {dir}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn exists(dir: &Utf8Path) -> bool {
    dir.join("HEAD").is_file()
}

/// Make an empty bare repo at `dir`, `main` its default branch. Making one that
/// exists is a no-op.
pub fn create(dir: &Utf8Path) -> ApiResult<()> {
    if exists(dir) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let output = Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(dir.as_str())
        .output()?;
    if !output.status.success() {
        return Err(HttpError::internal(format!(
            "git init {dir}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    for (key, value) in REPO_CONFIG {
        checked(dir, &["config", key, value])?;
    }
    Ok(())
}

pub fn delete(dir: &Utf8Path) -> ApiResult<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir)?;
    Ok(true)
}

/// A file's text at `reference`, or `None` when there is no such ref or path.
pub fn read_file(dir: &Utf8Path, reference: &str, path: &str) -> Option<String> {
    if !exists(dir) {
        return None;
    }
    let output = git(dir, &["cat-file", "blob", &format!("{reference}:{path}")]).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The commit `refs/heads/<branch>` names, in `namespace` when given.
pub fn branch_head(dir: &Utf8Path, namespace: Option<&str>, branch: &str) -> Option<String> {
    if !exists(dir) {
        return None;
    }
    let prefix = namespace.map(namespace_prefix).unwrap_or_default();
    let output = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{prefix}refs/heads/{branch}^{{commit}}"),
        ],
    )
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn has_commit(dir: &Utf8Path, commit: &str) -> bool {
    exists(dir)
        && git(dir, &["cat-file", "-e", &format!("{commit}^{{commit}}")])
            .is_ok_and(|output| output.status.success())
}

/// Whether `ancestor` is `descendant` or behind it.
pub fn is_ancestor(dir: &Utf8Path, ancestor: &str, descendant: &str) -> bool {
    git(dir, &["merge-base", "--is-ancestor", ancestor, descendant])
        .is_ok_and(|output| output.status.success())
}

/// One ref update for [`update_refs`]: `old` none means "must not exist".
pub struct RefUpdate<'a> {
    pub reference: &'a str,
    pub new: &'a str,
    pub old: Option<&'a str>,
}

/// Move refs all together or not at all (`git update-ref --stdin`, in one
/// transaction). Returns whether they moved; a ref not at its `old` moves none.
pub fn update_refs(dir: &Utf8Path, updates: &[RefUpdate<'_>]) -> ApiResult<bool> {
    let mut script = String::from("start\n");
    for update in updates {
        let old = update
            .old
            .unwrap_or("0000000000000000000000000000000000000000");
        script.push_str(&format!(
            "update {} {} {old}\n",
            update.reference, update.new
        ));
    }
    script.push_str("prepare\ncommit\n");
    run_stdin(dir, &["update-ref", "--stdin"], &script)
}

/// Point every ref in `refs` at its commit, whatever was there.
pub fn set_refs(dir: &Utf8Path, refs: &[(String, String)]) -> ApiResult<()> {
    if refs.is_empty() {
        return Ok(());
    }
    let script: String = refs
        .iter()
        .map(|(reference, commit)| format!("update {reference} {commit}\n"))
        .collect();
    if run_stdin(dir, &["update-ref", "--stdin"], &script)? {
        Ok(())
    } else {
        Err(HttpError::internal(format!("could not set refs in {dir}")))
    }
}

/// Delete every ref under `prefix`.
pub fn delete_refs_under(dir: &Utf8Path, prefix: &str) -> ApiResult<usize> {
    if !exists(dir) {
        return Ok(0);
    }
    let refs = list_refs(dir, prefix)?;
    let script: String = refs
        .iter()
        .map(|(reference, _)| format!("delete {reference}\n"))
        .collect();
    if !refs.is_empty() && !run_stdin(dir, &["update-ref", "--stdin"], &script)? {
        return Err(HttpError::internal(format!(
            "could not delete {prefix} in {dir}"
        )));
    }
    Ok(refs.len())
}

/// Every ref under `prefix`, with its object.
pub fn list_refs(dir: &Utf8Path, prefix: &str) -> ApiResult<Vec<(String, String)>> {
    let listed = checked(
        dir,
        &["for-each-ref", "--format=%(refname) %(objectname)", prefix],
    )?;
    Ok(listed
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(reference, object)| (reference.to_owned(), object.to_owned()))
        .collect())
}

fn run_stdin(dir: &Utf8Path, args: &[&str], input: &str) -> ApiResult<bool> {
    let mut child = Command::new("git")
        .arg("--git-dir")
        .arg(dir.as_str())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    Ok(output.status.success())
}

/// A tracker commit's pins: `.newgit/pins/<name>` gitlinks, by child name.
pub fn pins_of(dir: &Utf8Path, commit: &str) -> Vec<(String, String)> {
    let Ok(output) = git(dir, &["ls-tree", &format!("{commit}:.newgit/pins")]) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (meta, name) = line.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            (fields.next()? == "160000").then_some(())?;
            fields.next()?;
            Some((name.to_owned(), fields.next()?.to_owned()))
        })
        .collect()
}
