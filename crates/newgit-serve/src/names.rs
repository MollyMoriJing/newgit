//! Names: projects, repos, and where each repo lives on disk.
//!
//! The API still names repos as the Worker did (`<owner>.<project>.t.<tracker>`,
//! `.t.<tracker>.<login>`, `.i.<instance>.<tracker>`), so the CLI is unchanged.
//! On disk they are laid out by home, so a project's state and repos are one
//! directory: `homes/<owner>/<project>/`. That directory is the unit a
//! second node would take over.
//!
//! An instance's fork of a tracker is not a repo of its own: it is a Git
//! namespace (`refs/namespaces/i/refs/namespaces/<instance>/`) inside the
//! tracker's repo, so making one copies refs, not objects. A private
//! instance's forks are the exception: separate repos, because a shared
//! object store answers "do you have this?" for everyone in it.

use camino::{Utf8Path, Utf8PathBuf};

/// A project, as `<owner>/<name>`. The owner is a GitHub login, lowercased.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId {
    pub owner: String,
    pub name: String,
}

impl ProjectId {
    /// `<owner>/<name>`: the project's key, and its home.
    pub fn key(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The project's directory: its repos.
    pub fn dir(&self, data: &Utf8Path) -> Utf8PathBuf {
        data.join("homes").join(&self.owner).join(&self.name)
    }

    /// The prefix every repo name of this project starts with.
    pub fn repo_prefix(&self) -> String {
        format!("{}.{}.", self.owner, self.name)
    }
}

pub fn parse_project_key(key: &str) -> Option<ProjectId> {
    let mut parts = key.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || !is_part(owner) || !is_part(name) {
        return None;
    }
    Some(ProjectId {
        owner: owner.to_lowercase(),
        name: name.to_owned(),
    })
}

/// One repo, as the API names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoRef {
    Tracker {
        project: ProjectId,
        tracker: String,
        /// A per-user tracker's content for one user.
        login: Option<String>,
    },
    Fork {
        project: ProjectId,
        instance: String,
        tracker: String,
    },
}

impl RepoRef {
    pub fn tracker(project: &ProjectId, tracker: &str) -> Self {
        Self::Tracker {
            project: project.clone(),
            tracker: tracker.to_owned(),
            login: None,
        }
    }

    pub fn project(&self) -> &ProjectId {
        match self {
            Self::Tracker { project, .. } | Self::Fork { project, .. } => project,
        }
    }

    pub fn tracker_name(&self) -> &str {
        match self {
            Self::Tracker { tracker, .. } | Self::Fork { tracker, .. } => tracker,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Tracker {
                project,
                tracker,
                login: None,
            } => format!("{}t.{tracker}", project.repo_prefix()),
            Self::Tracker {
                project,
                tracker,
                login: Some(login),
            } => format!("{}t.{tracker}.{login}", project.repo_prefix()),
            Self::Fork {
                project,
                instance,
                tracker,
            } => format!("{}i.{instance}.{tracker}", project.repo_prefix()),
        }
    }
}

pub fn parse_repo_name(name: &str) -> Option<RepoRef> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.iter().any(|part| !is_part(part)) {
        return None;
    }
    let project = ProjectId {
        owner: parts.first()?.to_lowercase(),
        name: (*parts.get(1)?).to_owned(),
    };
    match &parts[2..] {
        ["t", tracker] => Some(RepoRef::Tracker {
            project,
            tracker: (*tracker).to_owned(),
            login: None,
        }),
        ["t", tracker, login] => Some(RepoRef::Tracker {
            project,
            tracker: (*tracker).to_owned(),
            login: Some(login.to_lowercase()),
        }),
        ["i", instance, tracker] => Some(RepoRef::Fork {
            project,
            instance: (*instance).to_owned(),
            tracker: (*tracker).to_owned(),
        }),
        _ => None,
    }
}

/// A repo name given as itself or as a remote URL ending in it.
pub fn repo_from_url_or_name(value: &str) -> &str {
    if !value.contains('/') {
        return value;
    }
    let last = value.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    last.strip_suffix(".git").unwrap_or(last)
}

/// Where Git finds a repo: its directory, and for a fork the namespace in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub git_dir: Utf8PathBuf,
    pub namespace: Option<String>,
}

/// A tracker's own repo on disk.
pub fn tracker_dir(data: &Utf8Path, project: &ProjectId, tracker: &str) -> Utf8PathBuf {
    project.dir(data).join("t").join(format!("{tracker}.git"))
}

/// Where `repo` is. `private` says whether a fork's instance is private: its
/// forks are repos of their own.
pub fn locate(data: &Utf8Path, repo: &RepoRef, private: bool) -> Location {
    match repo {
        RepoRef::Tracker {
            project,
            tracker,
            login: None,
        } => Location {
            git_dir: tracker_dir(data, project, tracker),
            namespace: None,
        },
        RepoRef::Tracker {
            project,
            tracker,
            login: Some(login),
        } => Location {
            git_dir: project
                .dir(data)
                .join("u")
                .join(tracker)
                .join(format!("{login}.git")),
            namespace: None,
        },
        RepoRef::Fork {
            project,
            instance,
            tracker,
        } if private => Location {
            git_dir: project
                .dir(data)
                .join("i")
                .join(instance)
                .join(format!("{tracker}.git")),
            namespace: None,
        },
        RepoRef::Fork {
            project,
            instance,
            tracker,
        } => Location {
            git_dir: tracker_dir(data, project, tracker),
            namespace: Some(fork_namespace(instance)),
        },
    }
}

/// The Git namespace an instance's forks live in, inside each tracker's repo.
pub fn fork_namespace(instance: &str) -> String {
    format!("i/{instance}")
}

/// Where `namespace`'s refs are stored, as Git lays them out.
pub fn namespace_prefix(namespace: &str) -> String {
    namespace
        .split('/')
        .map(|part| format!("refs/namespaces/{part}/"))
        .collect()
}

/// Whether `value` may be one part of a repo name: letters, digits, `_` or
/// `-`, starting with a letter or digit, at most 63 characters. Every path
/// on disk is joined only from parts that pass this.
pub fn is_part(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

pub fn part_problem(what: &str, value: &str) -> Option<String> {
    (!is_part(value)).then(|| {
        format!(
            "{what} `{value}` cannot name a repo on the remote: use letters, digits, `_` or `-` (no `.`), starting with a letter or digit, at most 63 characters"
        )
    })
}

/// The tracker a tracker id is mounted in: `t3code__repos` in `t3code`,
/// `t3code` in `root`; none for `root`.
pub fn parent_of(id: &str) -> Option<&str> {
    if id == "root" {
        return None;
    }
    Some(id.rfind("__").map_or("root", |at| &id[..at]))
}

/// `id` and every tracker above it, nearest first, ending at `root`.
pub fn lineage(id: &str) -> Vec<&str> {
    let mut chain = vec![id];
    let mut at = id;
    while let Some(up) = parent_of(at) {
        chain.push(up);
        at = up;
    }
    chain
}

/// Whether `id` is `anchor` or mounted, however deep, inside it.
pub fn within(id: &str, anchor: &str) -> bool {
    lineage(id).contains(&anchor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_names_round_trip() {
        let project = parse_project_key("Jack/newgit").expect("a key");
        assert_eq!(project.key(), "jack/newgit");
        for name in [
            "jack.newgit.t.root",
            "jack.newgit.t.env.ana",
            "jack.newgit.i.fix-1.root",
        ] {
            assert_eq!(parse_repo_name(name).expect(name).name(), name);
        }
        assert_eq!(parse_repo_name("jack.newgit.x.root"), None);
        assert_eq!(parse_repo_name("jack.newgit.t.../etc"), None);
        assert_eq!(
            repo_from_url_or_name("http://h/_git/jack.newgit.t.root.git"),
            "jack.newgit.t.root"
        );
    }

    #[test]
    fn forks_are_namespaces_unless_private() {
        let data = Utf8Path::new("/d");
        let fork = parse_repo_name("jack.newgit.i.fix.root").expect("a fork");
        let shared = locate(data, &fork, false);
        assert_eq!(shared.git_dir, "/d/homes/jack/newgit/t/root.git");
        assert_eq!(shared.namespace.as_deref(), Some("i/fix"));
        assert_eq!(
            namespace_prefix("i/fix"),
            "refs/namespaces/i/refs/namespaces/fix/"
        );
        let private = locate(data, &fork, true);
        assert_eq!(private.git_dir, "/d/homes/jack/newgit/i/fix/root.git");
        assert_eq!(private.namespace, None);
    }

    #[test]
    fn lineage_follows_ids() {
        assert_eq!(lineage("a__b"), vec!["a__b", "a", "root"]);
        assert!(within("a__b", "a"));
        assert!(!within("a", "a__b"));
    }
}
