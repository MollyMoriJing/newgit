- `undo` no longer marks a resource `ready` just because its restore had
  nothing to run, so one whose `prepare` failed, or never ran behind a
  failed dependency, stays `failed` or `pending` after an undo.

  A `none` or `external` restore, a resource with no `[restore]`, and a
  `recompute` skipped on unchanged identity all succeed without running
  anything, and undo took that as proof the resource was up: `newgit
  status` showed it and its instance `ready`, its dependents stopped
  counting as blocked, and `spawn --from-checkpoint`, which ends in the
  same undo, recorded a `prepare` that had just failed as `ready`. Such a
  restore now leaves the status as it was; one that runs a command still
  sets `ready` or `failed` by its result. The warning about a failed
  resource the safety checkpoint skips no longer blames a restore when it
  was the `prepare` that failed. A resource an earlier undo already marked
  `ready` stays so until `newgit action <resource>.prepare` runs again.
