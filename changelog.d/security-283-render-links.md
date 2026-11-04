- A render no longer writes through a link
  ([#283](https://github.com/Spheroman/newgit/issues/283)).

  A `[[render]]` wrote this instance's ports and exports into its file in
  place. It followed a symlink at the path or at a directory above it, and
  wrote into a hard link there. So once anything put a link there (you, a
  script, a tool), `resource rebind`, `tracker use`, and the re-render
  after `undo`, `tracker pull`, or `tracker checkout` rewrote whatever the
  link pointed at, even a file outside the workspace, and a dangling link
  created its target. Taking the values back out, before a capture or
  when a resource is unbound, wrote through a hard link or a symlinked
  directory the same way. Now the render refuses a symlink at the path or
  above it before writing any of the resource's files, naming the path,
  the link, and its target; a link that stays inside the workspace is
  refused too, since it reaches some other file. And every write of a
  rendered file replaces it with a new file, so a hard link's other names
  keep what they held. One consequence: a container that bind-mounts a
  single rendered file keeps the old file until it restarts. If a render
  now fails on a link you meant, replace the link with the real file or
  directory, and `newgit resource rebind` renders it again.
