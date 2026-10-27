- `undo` no longer runs a resource's `[restore] command` with nothing to
  restore from, after a checkpoint that went ahead without the resource's
  dump or handle because it was not up. With any bundled database
  template, that restore emptied the database once it was back up. This
  was disclosed under "Not in this PR" in
  [#137](https://github.com/Spheroman/newgit/pull/137).

  A checkpoint goes ahead without a resource that is not up and whose
  capture fails, and `newgit checkpoint` says an undo to it "leaves them as
  they are". The undo ran that resource's `[restore] command` all the same,
  with `{{state_ref}}` unrendered, and every bundled database template
  drops or resets the database before it reads the dump. `spawn
  --from-checkpoint`, which ends in the same undo, ran it in the new
  instance, and a redo did the same to a resource that was failed when the
  undo it redoes began, since that undo's safety checkpoint skips it. When
  the checkpoint is `command` or `external` and the restore is a `command`,
  the undo now leaves that restore out: the resource keeps its state and
  status, and a warning names it. A process it was running is still
  stopped with the rest and started again after its dependencies, and a
  restart that fails is reported like any other. That holds for
  checkpoints taken before this fix too. A `recompute` restore still
  rebuilds such a resource, since it ignores the ref. `newgit undo` prints
  no `resource:` line for a restore it leaves out, `spawn
  --from-checkpoint --json` reports that resource's `action` and `ok` as
  null, and `checkpoint --verify` reports a resource it could not capture
  as `NOT CAPTURED` when its restore runs something, and does not call the
  instance verified.

  A `command` restore whose `{{state_ref}}` has no value in the checkpoint,
  such as one behind a `command` checkpoint that printed nothing, is now
  refused rather than handed the literal placeholder, and the undo reports
  it as a failed restore.
