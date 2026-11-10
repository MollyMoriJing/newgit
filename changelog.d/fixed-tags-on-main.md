- A tag on a commit of `main` is made, not refused as off `main`.

  The remote asked Artifacts for each branch's head as `refs/heads/main`,
  which it resolves to nothing, so every branch read as having no head. A
  tag pushed from the project root, or made by a GitHub release, was
  refused for every commit, the tip of `main` included, saying to push
  `main` first. A merge took every pin it moved as off that tracker's
  `main`: a move along it, root's maintainers' to make, was asked of the
  tracker's maintainers instead, refusing someone who maintains only root,
  and the merge was handed a write to the tracker it did not need. Trackers'
  definitions were read again on every request, never kept. The head is now
  asked for by the branch's name. A release tagged on GitHub before tags
  came back from mirrors comes in when its webhook event is redelivered.
