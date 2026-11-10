- `newgit serve` runs a newgit remote as one binary: Git repositories on
  the machine's own disk, project state in one SQLite database, and the
  same API newgit.dev answers, so the CLI works against either
  ([#344](https://github.com/Spheroman/newgit/issues/344)).

  The Worker's slowness was mostly Artifacts and the hops around it. On one
  machine, without them, `remote add` takes 0.85 s (9.4 s on newgit.dev),
  a plain `git clone` 0.4 s, a checkpoint and push 1.7 s (15 s), a merge
  1.2 s (6.4 s), and `spawn --from-checkpoint` 1.1 s (8.4 s). Those are
  local numbers, with no internet round trips. A fork is a Git namespace
  inside the tracker's repo, so making one costs nothing, and the
  password for every repo is the caller's own newgit token, checked on
  each request. It does not take GitHub webhooks yet, and newgit.dev still
  runs on the Worker until moving its data across is designed and tested.
