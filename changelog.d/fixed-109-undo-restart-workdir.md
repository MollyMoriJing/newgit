- `undo` restarts a long-running action in its `workdir`, as `newgit
  action` does, rather than in the workspace root
  ([#109](https://github.com/Spheroman/newgit/issues/109)).

  The restart built its own invocation and handed the supervisor the
  workspace root, so a dev server with `workdir = "apps/web"` came back in
  the wrong directory after an undo, where `npm run dev` fails or runs the
  wrong package. newgit reported it restarted, because the supervisor only
  tracks liveness. Both paths now resolve the directory the same way, and
  a `workdir` the undo removed is reported as a failed restart.
