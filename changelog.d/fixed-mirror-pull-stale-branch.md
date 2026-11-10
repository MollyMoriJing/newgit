- A pull request merged on a mirror from a branch made before `main` moved a
  tracker the mirror withholds now lands, with `main`'s pin. The pull
  refused it as "merges two lines of `t3code`" unless one side's whole commit
  was an ancestor of the other, so any pull request branched before a
  `newgit pull t3code`, a Dependabot bump among them, was refused, and every
  webhook pull queued behind it waited. It now asks which side changed that
  tracker since the two forked, as Git's three-way merge would, and refuses
  only when both did.
