- A checkpoint no longer fails on a new file in a dropped code tracker's
  paths, and `newgit undo` no longer removes an edit there that no
  checkpoint holds.

  A dropped tracker's paths are outside the workspace's sparse checkout,
  where the `git add -A` that a checkpoint snapshots with skips an edited
  file without a word and fails on a new one. So a file written into a
  dropped tracker's directory made `newgit checkpoint` fail with Git's
  "outside of your sparse-checkout definition" advice, and with it each
  command that takes a checkpoint first: `undo`, `merge`, `checkpoint
  --verify`, a `git push` from the workspace, and a `tracker drop --discard`
  with work to discard. An edit to one of that tracker's files was left out
  of the checkpoint without a word, and the next `undo` that restored the
  source removed it. The snapshot now leaves the dropped trackers' paths out
  by name, and still holds the changes to a bound tracker nested in one. A
  checkpoint that leaves changes out warns, naming each under its tracker,
  which `newgit checkpoint`, `merge`, `undo --only` and `git push` print; a
  file a resource renders is not counted, since undo renders it again.
  `undo` without `--only` refuses while there are any, before it changes
  anything, since it would remove them; it says to bind the tracker again
  with `newgit tracker use`, after which a checkpoint holds them.
