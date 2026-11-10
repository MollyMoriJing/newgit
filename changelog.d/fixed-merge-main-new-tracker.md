- `newgit merge main` brings in a tracker in Git that was carved in the
  project root after it was linked.

  The merge seeded the new tracker's repo with its commit, then uploaded
  that commit again ahead of the merge. The remote reads each tracker's
  definition from `access`'s `main`, which has no definition for the new
  tracker until this merge lands, so it refused the upload (404, "not
  defined"), and nothing could carve a tracker out of a linked project.
  Found carving `serve` and `cloudflare` out of newgit's own root.
