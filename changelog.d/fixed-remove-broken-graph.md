- `newgit remove`, and `newgit cleanup` finishing an instance whose
  workspace is gone, no longer refuse while the resource graph does not
  resolve. Teardown is the way out of a broken project, and 0.2.0 promised
  that `remove` would never depend on the graph holding together.

  The promise held only for instances with no `[cleanup] command` to run.
  A hook's environment came from the same assembly as `newgit run`'s,
  which refuses any graph that does not resolve, so one bad `depends_on`
  anywhere made every instance with a hook impossible to remove, even one
  that binds only valid resources. A hook now gets that environment layered
  in whatever order the graph has. An edge that does not resolve is skipped
  rather than fatal, so every variable is there.

  The order decides only a name two declarations claim, and that is where
  teardown still refuses: when both reach the instance with different
  values, a hook would get whichever is layered last and could tear down
  the wrong thing with it. An existing instance meets that when a port's
  `env` is set or renamed onto a name another of its resources already
  uses, since a port's `env` is read from its definition each time. The
  refusal names the collision and leaves the instance untouched until it
  is renamed. A collision that does not reach the instance, such as one
  with a resource added after the spawn, does not block it. `newgit env`
  still refuses on any graph that does not resolve, so it cannot show a
  teardown's environment beforehand.
