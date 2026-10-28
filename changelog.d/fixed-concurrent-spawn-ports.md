- Two `newgit spawn`s that overlap are no longer promised the same port.
  Agents spawning side by side is the case newgit is built for, and the
  instance that came up second could not bind its port, or reached the
  other instance's service on it.

  The binding records are the only claim on a port, and `spawn` wrote its
  record only after every `prepare` had run, so a spawn that started
  while another was still installing found no claim on the port that one
  had been given, and nothing listening on it yet either. Each resource's
  ports are now written into the record before its `prepare` runs, under
  a lock (`.newgit/local/ports.lock`) held only while they are chosen, so
  spawns still install side by side. `tracker use`, and an `undo` that
  binds resources, claim their ports the same way.

  The record is now written as soon as an instance's resources start to
  bind, not once they are prepared: `newgit status` shows an instance
  that is still spawning, and one whose spawn failed or was killed after
  that point keeps its record, and with it its ports, until `newgit
  remove`. Each resource is in that record, `Pending`, with its exports
  and rendered files, before its `prepare` runs, so after a spawn killed
  in `prepare`, `newgit action <resource>.prepare` runs it with the
  resource's exports, and newgit still knows which files it rendered.

  A resource is `Pending` in that record while its `prepare` runs, so
  readiness no longer assumes a `Pending` resource is waiting on a
  dependency, where it said it "never came up (a dependency of it did
  not)": with no dependency blocking it, it says the resource "has not
  been prepared" and to run `newgit action <resource>.prepare` unless
  newgit is still preparing it.
