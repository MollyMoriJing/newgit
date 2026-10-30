- `undo` no longer writes a checkpoint's uncommitted changes into the paths
  of a code tracker it leaves dropped
  ([#225](https://github.com/Spheroman/newgit/issues/225)).

  Undo reapplied the uncommitted state by checking the snapshot's tree out
  over the whole workspace. That writes each path where the tree differs
  from the index, outside the sparse checkout too, and clears that path's
  skip-worktree bit, so a change the snapshot held in a dropped tracker's
  paths came back as an ordinary tracked file of a tracker the instance
  does not bind. A snapshot holds one when, for instance, the dropped
  tracker's definition has since taken over a path the checkpoint changed.
  What the sparse checkout leaves out is now reapplied as the checkpoint's
  HEAD has it, so it stays out of the working tree; the rest of the
  snapshot, a bound tracker nested in a dropped one included, is reapplied
  as before. `spawn --from-checkpoint` ends in the same undo. Nothing
  either command prints changes.
