- A render no longer writes through a symlink
  ([#283](https://github.com/Spheroman/newgit/issues/283)).

  A `[[render]]` rewrites its file in place with this instance's ports and
  exports, and it followed a link at the path or at a directory above it.
  So once anything put a link there (you, a script, a tool), `resource
  rebind`, `tracker use`, and the re-render after `undo`, `tracker pull`,
  or `tracker checkout` rewrote whatever the link pointed at, even a file
  outside the workspace, and a dangling link created its target. Now the
  render refuses such a path before writing any of the resource's files,
  naming the path, the link, and its target; a link that stays inside the
  workspace is refused too, since it reaches some other file. Taking the
  values back out, before a capture or when a resource is unbound, leaves
  a file reached through a link alone. If a render now fails on a link you
  meant, replace the link with the real file or directory, and `newgit
  resource rebind` renders it again.
