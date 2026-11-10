- A GitHub merge is no longer refused because GitHub's API could not be
  reached ([#392](https://github.com/Spheroman/newgit/issues/392)).

  To find whose withheld work a merged commit carries, the webhook asks
  GitHub which pull request it came from. When that failed, every instance
  holding withheld work counted, so on 2026-11-09 half an hour of
  api.github.com being out of reach refused two merges that could lose
  nothing, telling people to `newgit merge` an instance whose branch was not
  on GitHub yet. Now only an instance GitHub names counts. Until GitHub
  answers, the pull waits, asking again with a growing gap, spends no
  attempt, and `newgit status` says why. After two hours it gives up saying
  so, and says to redeliver the event or `newgit pull --remote <target>`. A
  refusal for an instance GitHub did name gives both ways on, not only
  `newgit merge`. A failed attempt is now retried up to ten times, waiting
  longer each time (about 45 minutes in all, where five tries used to take
  about a minute), so a `Network connection lost` from the endpoint during a
  deploy or an outage is retried until the pull finishes.
