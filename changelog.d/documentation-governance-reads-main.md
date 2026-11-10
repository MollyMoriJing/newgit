- The remote's design document now says where governance is read from:
  each definition holder's `main`, never a branch's own copy, so a branch
  cannot widen its own audience or maintainers.

  It said "the branch's own `access`", which is not what the Worker or
  `newgit serve` do, and not what was meant. One consequence: a tracker
  created in a merge is not known to the remote until that merge lands its
  definition on `main`.
