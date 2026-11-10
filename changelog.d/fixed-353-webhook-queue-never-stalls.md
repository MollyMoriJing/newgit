- A GitHub merge answered 202 always lands or says why it did not
  ([#353](https://github.com/Spheroman/newgit/issues/353)).

  The queue of webhook pulls is moved on only by the project's alarm, which
  set its next alarm last. Anything that threw first (a deploy resetting the
  object mid-step, Artifacts down during the sweep) left the pull queued
  with nothing to move it, until another event happened to set an alarm,
  which is why redelivering the event "fixed" it. The alarm is now set
  before any work, and nothing it runs can stop it. Asking for the
  project's state, as `newgit status` does, also starts a queue that has
  none and says it was stalled. A pull that waited two hours without a
  failed attempt used to be dropped without a word; every pull that ends is
  now kept, with why. `newgit status` says what a waiting pull waits on and
  when a failed one is tried again, and every pull refused or given up since
  its target last landed, until the mirror's commits are on `main`.
