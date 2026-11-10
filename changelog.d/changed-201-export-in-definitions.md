- Where a tracker ships is now said by its own definition, with `export`,
  and `.newgit/exports/` is gone
  ([#201](https://github.com/Spheroman/newgit/issues/201)). A target listed
  its trackers, so carving a directory out of a mirrored tracker silently
  dropped it from the mirror until someone added it by hand, which nearly
  happened to `serve` and `cloudflare`. Now root says
  `export = { url = "github.com/acme/widget", branches = true }`, a child
  says `export = "inherit"` to ship wherever its parent ships, a list ships
  one tracker in several places (`["inherit", "github.com/acme/lib"]`), and
  no key means it never ships. `tracker create --git --path` writes
  `"inherit"`, unless the new tracker's audience is narrower than its
  parent's. A target declared by a tracker other than root is rooted at it.
  Definitions live in `access`, so widening where anything ships is now an
  owner's merge.

  Upgrading: move each `.newgit/exports/<name>.toml` into the definitions.
  A target with `root = "<t>"` becomes `export` in `<t>`'s definition, and
  `"inherit"` goes on every tracker it mounts; a target listing `trackers`
  becomes `export` in root's and `"inherit"` in each other one. `url`,
  `branch` and `branches` move as they are. A target is named after its
  repository now; add `name = "github"` to keep an old name. A bare
  `owner/repo` URL is refused: write `github.com/owner/repo`. A
  `public` tracker no longer ships just for being public. The GitHub
  webhook finds its target by the repository the event came from, so its
  URL needs no `&target=`. The endpoint must be deployed before a project
  whose definitions say `export` lands anything: older ones refuse the key.
- A project's tags go only to the export target root declares, never to one
  rooted at a child tracker. That target is the child's own history with
  versions of its own (t3code's `v0.1.x`), and a tag never moves, so a
  project's `v0.9.0` there would take that name from it for good. Tags from
  such a target were already refused coming in.
