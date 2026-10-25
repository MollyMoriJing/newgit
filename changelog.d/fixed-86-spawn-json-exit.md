- `spawn --json` now exits non-zero when a resource did not come up, and
  says which: the follow-up to
  [#86](https://github.com/Spheroman/newgit/issues/86), which fixed only
  the human-readable `spawn`.

  #86's check ran only after the `--json` branch had printed and returned,
  and that branch printed nothing about resources, so a failed `prepare`
  exited 0 with nothing in the output saying so — to a script, the caller
  most likely to ask for JSON and the one that goes by the exit code. Each
  resource is now listed with its `status` (`ready`, `failed`, `blocked` or
  `pending`), its ports, its `prepare` log, `blocked_by`, and any render or
  export error, and the exit is the same `exit 1`, after the JSON; the
  instance and its record are kept, as they are without `--json`.
  `spawn --from-checkpoint --json` had the same gap, and now judges by the
  instance the restore leaves: a failed restore exits 1, and so does a
  resource the checkpoint never had that failed to bind. Its `resources`
  lists every resource that instance has, with `action` and `ok` null for
  one the restore did not touch, and `recovery_record` names the record of
  a failed restore.
