- `newgit undo` now brings back an uncommitted deletion of a tracked file.

  A checkpoint promises the worktree as it stood, uncommitted changes
  included, and its snapshot did record the deletion. But undo first reset
  hard to the checkpoint's commit, which put the file back, and then laid
  the snapshot over it, writing every path the snapshot had and removing
  none it lacked. The file came back with its committed content and a
  clean `git status`: work resumed from a state the instance was never in,
  and nothing showed it. With every tracked file deleted, undo failed
  instead, after the reset had already run. Checkpoints taken before this
  fix recorded their deletions too; undoing to one now restores them.
