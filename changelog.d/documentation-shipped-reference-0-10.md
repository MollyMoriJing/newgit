- `newgit reference`, `--help`, and the wiki say what 0.10.0 actually does.

  An agent setting newgit up reads these cold, and several of them were
  wrong in ways it would act on. They called newgit.dev invite-only, when it
  has taken any GitHub account since 2026-11-02 (up to 5 projects each,
  paused while over its weekly budget). They counted three built-in trackers
  where `init` makes two, and never said that trackers nest or are named by
  path (`t3code/repos`, `..`, `/remote/<owner>/<project>/...`). They said
  `tracker link` makes a symlink, which it does not, and that
  `--merge-with-source` carries a tracker through a merge, when nothing reads
  it. In the resource sections: the port probe was said to check only
  `127.0.0.1` (it checks all four addresses), the `external` template's
  captures had the wrong names, `key_command`'s environment and working
  directory were missing, and the lists of commands that take the instance
  lock or refuse a broken graph were short. Arguments that printed no help
  now have some.
