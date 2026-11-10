- `docs/design/landing.md` says how changes will land on `newgit serve`:
  `merge` lands or doesn't, every view shows it when it returns, refusals
  name a real conflict, and plain `git push` semantics hold. Every tracker
  keeps its own history, held in one object store per audience and never in
  a store shared across audiences. `anchoring.md` now describes the Worker
  as built. The nixpkgs-scale benchmark behind it, with its independent
  review, is in `docs/research/nixpkgs-bench.md`.
