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
  remove`.
