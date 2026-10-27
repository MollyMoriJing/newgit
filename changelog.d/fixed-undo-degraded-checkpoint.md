- `undo` no longer restores a resource from a checkpoint that went ahead
  without its dump or handle because it was not up. It leaves the resource
  as it is, as `newgit checkpoint` says it will, where it used to run the
  restore with nothing to restore from: with any bundled database
  template, an undo once the database was back up emptied it. This was
  disclosed under "Not in this PR" in
  [#137](https://github.com/Spheroman/newgit/pull/137).

  A checkpoint goes ahead without a resource that is not up and whose
  capture fails, and `newgit checkpoint` says an undo to it "leaves them as
  they are". The undo ran that resource's `[restore] command` all the same,
  with `{{state_ref}}` unrendered, and every bundled database template
  drops or resets the database before it reads the dump. `spawn
  --from-checkpoint`, which ends in the same undo, ran it in the new
  instance, and a redo did the same to a resource that was failed when the
  undo it redoes began, since that undo's safety checkpoint skips it. Such
  a resource is now left as the undo finds it when its `[checkpoint]` is
  `command` or `external`: not stopped, restored, or restarted, with its
  status unchanged and a warning naming it. That holds for checkpoints
  taken before this fix too. `newgit undo` prints no `resource:` line for
  it, `spawn --from-checkpoint --json` reports its `action` and `ok` as
  null, and `checkpoint --verify` reports it `NOT CAPTURED` when its
  restore runs something and does not call the instance verified.

  A `command` restore whose `{{state_ref}}` has no value in the checkpoint,
  such as one behind a `command` checkpoint that printed nothing, is now
  refused rather than handed the literal placeholder, and the undo reports
  it as a failed restore.
