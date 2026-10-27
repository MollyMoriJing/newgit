- `spawn --json` now exits non-zero when a resource did not come up, and
  says which: the follow-up to
  [#86](https://github.com/Spheroman/newgit/issues/86), which fixed only
  the human-readable `spawn`.

  #86's check ran only after the `--json` branch had printed and returned,
  and that branch printed nothing about resources, so a failed `prepare`
  exited 0 with nothing in the output saying so — to a script, the caller
  most likely to ask for JSON and the one that goes by the exit code. Each
  resource is now listed, by name, with its `status` (`ready`, `failed`,
  `blocked` or `pending`), its ports, its `prepare` log, any render or
  export error, and `blocked_by`: the dependencies that are not ready, for
  a resource that is not ready itself. The exit is the same `exit 1`,
  after the JSON; the instance and its record are kept, as they are
  without `--json`.

  `spawn --from-checkpoint --json` had the same gap, and now judges by the
  instance the restore leaves: a failed restore exits 1, and so does a
  resource the checkpoint never had that failed to bind. Status and
  `blocked_by` describe that instance, not the bind the restore ran after,
  and `recovery_record` names the record of a failed restore. Its
  `resources` array changed shape, which matters to a script that reads
  it: it now lists every resource the instance has, by name rather than
  in restore order, and `action` and `ok`, before always a string and a
  bool, are null for a resource the restore did not touch. A script that
  requires every `ok` to be true now fails on such a resource unless it
  skips the nulls.
