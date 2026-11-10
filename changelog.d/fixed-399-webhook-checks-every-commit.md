- A mirror commit refused for an instance's withheld work is no longer
  landed by the next GitHub event's pull
  ([#399](https://github.com/Spheroman/newgit/issues/399)).

  A webhook pull lands every mirror commit beyond `main`, but GitHub was
  only asked which pull request a commit came from for the commits in that
  event's own payload. A commit refused once stays on the mirror, so the
  next push there carried it in unasked, and the instance's work in a
  tracker the mirror does not ship was lost after all. Now the look the
  container makes before landing says every commit the pull would bring in,
  and that whole walk is checked: a GitHub merge commit's subject speaks
  for itself and the commits it merged, and GitHub is asked about the rest.
  Later pulls are refused the same way until `newgit merge <instance>` puts
  the work on `main` (or `newgit pull --remote <target>` lands only what the
  mirror has); nothing remembers the refusal, so the first pull after that
  lands. A walk of more than 20 commits no merge commit accounts for gives
  up at once, as an event of more than 20 commits did.
