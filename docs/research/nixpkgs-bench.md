# nixpkgs-scale benchmark: inline vs separate repos vs pooled

*Research note, 2026-11-10. Measured on one Mac (numbers are relative, not server capacity). Raw outputs were in `~/newgit-bench/results/`, scripts in `~/newgit-bench/scripts/`. Feeds [landing.md](../design/landing.md).*

> **Review (independent rerun, read this first).** The headline numbers reproduce within 2x. Three corrections override the text below:
>
> 1. **The tree-wide change is a tie, not a pooled win.** Inline's 2.6–3.7 s includes writing 5,000 split commits; pooled's "landing only" excludes its 5,000 package commits. Like for like, the landing step is 0.29–1.07 s (inline) against 0.58–1.3 s (pooled); all the server's work is 2.6–3.7 s against 7–8 s.
> 2. **`newgit serve` does enable `uploadpack.allowFilter`** (`crates/newgit-serve/src/repos.rs`). The "full clone, 3.2 GB" row is an artifact of the bench's own repos, not of serve.
> 3. **A per-fetch reachability check does not make a mixed-audience pool safe.** Under protocol v2 any object can be fetched by id; a `have` line answers `ACK` for a hidden commit (an existence oracle); and a crafted push to `receive-pack` created a ref to a hidden commit with an empty pack, which the proposed `rev-list <want> --not <reader refs>` check then allowed (attacks 1 and 2 of `gitnamespaces(7)`). So the rule is **one audience per pool**, with no check needed, and never a pool as a security boundary. `newgit serve` is not exposed today: every repo it serves has one audience.
>
> Also: separate's landing writes no root pin commit, which flatters its 522/s; a real pool holds root's ~3 GB history too, so its gc is close to inline's; and the build-time gap is 242 s against 11 s once the shared split work is set aside. On performance, pooled and inline-with-stored-splits are about even; pooled is chosen for the model (every tracker its own history, storage never changes shape). Not measured: many concurrent clones against one pool, conflicting landings, readers with thousands of refs.

## Verdict

**Three layouts compared**

- **inline**: every package in one repo and one history. Rooted views come from a stored subtree split or a partial clone.
- **separate**: one bare repo per package (22,287 repos).
- **pooled**: one bare repo. Each package's own history lives under `refs/trackers/<pkg>/main`. Root's history pins each package by gitlink. A landing is one `update-ref --stdin` transaction.

**Pooled is the best default for a home, with one condition.** It matches separate repos on everything a user touches: spawn, log and blame. On the server side it is level with or ahead of inline: landings are slightly faster and the tree-wide change clearly faster, while disk, gc and per-reader views come out about the same for the same content (my pooled view prototype is a bit slower, but that is an upper bound). It is far cheaper to run than 22k repos. The condition is that its shared object store needs a request-time reachability check (below), because Git does not provide one.

**Is inline "just one bloated monorepo"?**

- **For the server: no.**
  - All of nixpkgs history is 3.0 GB in one pack, and one `git gc` takes about 4.5 minutes.
  - The package part (`pkgs/by-name`) is about 0.28 GB of that.
  - The server holds one repo and about 22k extra refs for stored splits.
- **For users: yes, unless the server serves rooted views.**
  - A partial clone plus sparse checkout of `hello` took **99 s and 1.6 GB** with bitmaps (166 s without). The 1.6 GB is every tree of every commit. Its `git log` then took 5 s.
  - A stored subtree split took **0.13–0.18 s and 22 KB**. That is the same as separate repos (0.09–0.13 s, 22 KB).
  - So inline is fine for users only if the server maintains the split. Maintaining it costs about 14 ms per single landing, and under 1 ms per landing when batched.
- **Through `newgit serve` today, inline is worse than that.** The server does not enable `uploadpack.allowFilter`, so `clone --filter` silently becomes a full 3.2 GB clone (140 s).

**Where separate repos lose:**

- It takes 19.5 minutes to build, against 15.6 minutes for pooled.
- It has 424k files on disk, against 19 for pooled.
- Its gc takes 143 s wall (32 CPU-minutes), against 4–15 s for pooled.
- A 5,000-package change takes 57 s, against 7 s for pooled.
- It wins on only two things:
  - Isolation: a repo boundary needs no extra check.
  - Raw parallel ref moves: 522/s with 16 workers against pooled's 382/s. Serially, separate manages 145/s.

**Pooled is not a landslide over inline plus stored splits.** Landings are 382/s against 307–318/s. The tree-wide landing is 0.6–1.3 s against 2.6–3.7 s. Log and blame do not depend on Bloom filters. Inline keeps one composed history for free, which pooled does not. Pooled's real costs are:

1. The mandatory per-fetch check (below).
2. A 2.7 MB full ref advertisement for readers who list every namespace.
3. Independent per-package histories with no cross-package commits.

**Finding that matters for the design.** In Git 2.55 with protocol v2, `transfer.hideRefs` and `uploadpack.allowReachableSHA1InWant` do not stop a client fetching any object in a shared store by id. I verified this locally and through `newgit serve`. Protocol v0 does refuse. Pooled therefore needs its own check on every fetch's `want` lines:

- **What the check is.** `git rev-list --objects --use-bitmap-index <wants> --not <reader's refs>`; any output means refuse.
- **What it costs.** About 10–17 ms per fetch with a one-package reader, on top of roughly 100 ms.
- **How it scales.** For a want that is not a ref tip, the cost grows with the number of refs the reader may read:

| Reader's refs | Cost of the check |
|---|---|
| 1 | 6 ms |
| 1,000 | 43 ms |
| 20,000 | 660 ms |

- **Bitmaps matter.** Without them the walk gives wrong answers for blobs: it refuses ones that are reachable.
- **Tip wants are free.** Wants that are ref tips are an O(1) set lookup.
- **Not covered.** I did not measure or mitigate the `have`-line "do you have this?" oracle.

Private audiences should keep their own pool (own repo) in every layout, as `serve.md` decision 1 already says.

## Numbers

**Scale.** nixpkgs `master` at `c5b5c5edd3` (2026-11-10): 1,115,907 commits (418,244 first-parent) and **22,287 packages** under `pkgs/by-name/*/*`. Everything below was measured at full scale unless the **M/E** column says **E** (extrapolated). The 1k and 5k runs were only done for the build time. The split histories of all 22,287 packages total 228,130 commits, a median of 7 per package and a mean of 10.

Inline numbers are for the whole of nixpkgs. Separate and pooled hold only the `by-name` packages, so disk comparisons need the by-name-only inline estimate in the notes.

### Building the layouts

| | M/E | inline | separate | pooled |
|---|---|---|---|---|
| Get the data | M | clone from GitHub 215 s (3.02 GiB pack, 8.33M objects, full clone) | n/a | n/a |
| Commit-graph with changed-path Bloom | M | 123 s (85 MB); without Bloom 8.6 s (67 MB); incremental refresh 10 s | — | — |
| Per-package split histories, all 22,287 (shared by all three) | M | 926 s (16 workers) | 926 s | 926 s |
| Turn splits into the layout | M | fetch splits into the repo 27 s, 22,287 refs | 22,287 repos 242 s | 10.6 s (reftable) / 12.5 s (files) |
| **Total** | M | 215 s + 953 s with stored splits | **1,168 s (19.5 min)** | **937 s (15.6 min)** |
| Build at 1k / 5k packages | M | — | 56 s / 266 s | — |

Separate used about 4.7 CPU-hours over 16 workers. The split step is the same work for all three. It is 0.58 CPU-seconds per package of `git log -- <path>`. A single-pass tool would be cheaper, but I did not write one.

### The seven measurements

| # | Measurement | M/E | inline | separate | pooled |
|---|---|---|---|---|---|
| 1 | Rooted spawn of `hello`, stored split or namespace ref (24 commits, 2 files) | M | 0.13–0.18 s, 3 requests, 22.3 KB | clone 0.09–0.12 s, **21.9 KB**; init+fetch 0.13 s, 22.0 KB | 0.13–0.17 s, 22.0 KB (same with `hideRefs` or the check flag) |
| 1 | Same, via partial clone + sparse checkout | M | **99 s** with bitmaps (166 s without), 1.60 GB wire, 1.84 GB client `.git`; `git log` on the path afterwards 4.8 s | n/a | n/a |
| 1 | Partial clone + sparse + `git backfill --sparse` (blob history too) | M | 202 s, 1.61 GB | n/a | n/a |
| 1 | `--filter=tree:0` + sparse (history needs trees; not run) | M | 24–28 s, 521 MB | n/a | n/a |
| 1 | Through real `newgit serve` (it serves my repos as a project's tracker repo) | M | stored-split ref 0.16 s; `clone --filter` unsupported, full clone 140 s, 3.2 GB | 0.15 s | 0.15 s |
| 2 | Extend one package's split head per landing | M | p50 14 ms, p99 24 ms; spawn-bound (read ref 0.3, write commit 7, update-ref 7). Batched: ~0.7 ms extra per landing (58.7/s with, 61.3/s without) | n/a | n/a (the package head is the history; one more line in the transaction) |
| 3 | `git log` on one package, median of 24 packages | M | **309 ms** with Bloom; **3,497 ms** without. Stored split 12 ms | 6 ms | 6 ms |
| 3 | `git blame package.nix`, median of 24 | M | **348 ms** with Bloom; **2,807 ms** without. Stored split 17 ms | 7 ms | 7 ms |
| 4 | 100 concurrent landings, 1,000 per cell, batch of 100: throughput, p50 / p99 | M | merge-tree fold 59/s, 1.75 / 1.85 s; combine without a merge 307–318/s, 0.31–0.33 / 0.35 s (both with split updates) | 522/s, 0.19 / 0.22 s (16 parallel); serial 145/s, 0.68 / 0.79 s | **382/s, 0.26 / 0.28 s** (reftable); 350/s, 0.28 / 0.33 s (files) |
| 4 | Batch of 10 | M | merge-tree fold 50/s, 1.1 / 2.1 s; fast 135–141/s, 0.39–0.42 / 0.74–0.81 s | 255/s, 0.22 / 0.43 s | 132/s, 0.43 / 0.81 s (reftable); 152/s, 0.38 / 0.67 s (files) |
| 4 | Batch of 1 (300 landings) | M | 20–24/s | 39/s (serial) | 21–23/s |
| 5 | Tree-wide change, 5,000 packages: landing | M | 2.6 s (reftable) / 3.4 s (files). Moved main (a real merge): 2.6 / 3.7 s. One landing, 5,001 ref updates (main + splits) | **S1** (commits already in the 5,000 repos): 8.8 s, 5,000 ref moves. **S2** (server writes objects too): 56.9 s | landing only **0.6 s** (reftable) / 1.3 s (files). With object writes: 7.0 / 8.0 s |
| 6 | Disk after gc (APFS blocks) | M | 3.49 GB (pack 3.03 GB, idx 272 MB, bitmap 47 MB). By-name part ≈ 0.28 GB | 1.24 GB (383 MB of data; the rest is per-file block overhead) | **320 MB** (both ref backends) |
| 6 | Files and objects | M | 38 files, 8.56M objects | 424,267 files and dirs, 694,595 objects across 22,287 repos | **19 files**, 695,056 objects |
| 6 | Refs | M | 22,316 | 22,287 (one per repo) | 34,288 (22,287 real + 12,000 synthetic namespaces to reach 30k+) |
| 6 | gc / repack | M | `git gc` **267 s** (249 s user); repack with bitmap 111 s; commit-graph refresh 10 s | `git gc` in all repos, 16 at a time: **143 s wall**, 32 CPU-min | `git gc` **4.4 s** (reftable) / 14.5 s (files); repack with bitmap 1.9 s |
| 6 | Creating the 22k–34k refs; full ref advertisement | M | advert. 1.7 MB, 0.77 s | n/a | creation: reftable **0.24 s** / files **5.0 s**; advert. 2.67 MB, 0.99 s (reftable) / 1.78 s (files). Files backend leaves 70k loose ref files before gc (442 MB) |
| 7 | Mixed-reader view, first build, 3,000 commits, 1 / 3 / 10 audiences (200 private packages each; reader sees ⌊N/2⌋ of them; 200 / 400 / 1,000 packages hidden) | M | 7.0 / 6.6 / 6.6 s (2.3 ms/commit) | n/a | 10.6 / 11.8 / 12.0 s (3.5–4.0 ms/commit; upper bound, see notes) |
| 7 | Same, first build over the by-name era (254,748 first-parent commits) | **E** | ≈ 10 min, single thread | + ≈ 10 ms per package-touch (resolving from its repo): +38 min serial, ≈ 2.4 min at 16-way | ≈ 15–17 min |
| 7 | Incremental, per landing, p50 / p99 | M | 23 / 32–34 ms, flat across audiences | the inline figure + ≈ 10 ms per touched package (a `git rev-parse` in the package repo) | 29–32 / 38–43 ms, flat across audiences |
| — | Request-time reachability check (pooled), per fetch | M | n/a | n/a (a repo boundary needs none) | 10–17 ms added to a ≈ 100–120 ms fetch (tip 10, non-tip commit or blob 17); refusal verified. Direct walk by size of reader's set: 1 ref 6 ms; 1k refs 43 ms; 20k refs 660 ms; no-bitmap walk is wrong for blobs |

**Real landing sizes, for reference.** Since 2024-06-01, 105,482 first-parent commits touched `by-name`. 95% touch one package, the mean is 1.95, 90 touch more than 100, 13 touch more than 1,000, and the largest touched 11,238. Batch-of-1 and tree-wide numbers matter for that tail.

## Method

**Machine.** Apple M5 Pro, 18 cores, 48 GiB RAM, internal SSD (APFS), macOS 27.0.1, **git 2.55.0**, Python 3.15, 14 GiB of disk free at the start. The machine was otherwise idle for timed runs. One Mac, loopback network, no real network latency.

**Data.** I did a full clone of `https://github.com/NixOS/nixpkgs` (bare, `master` only, no `--filter`). The disk was tight (about 21 GB free), but 3.0 GiB fit. `pkgs/by-name/*/*` gave 22,287 packages. Subsets for the 1k and 5k builds are random with a fixed seed.

**Split tool.** `git filter-repo` is not installed, so I wrote a faster tool, `split_pkgs.py`:
- For each package, it takes `git log --parents --topo-order --reverse -- <path>`, which uses Git's own path-limited history simplification.
- It resolves each commit's subtree id with `cat-file --batch-check`.
- It emits a `fast-import` commit whose root tree is that existing subtree.
- No blob or tree is ever rewritten.
- Original author, committer and message are kept. Signatures and extra headers are dropped, so every commit id changes.
- A package's history starts where it first lived under `by-name`.

For correctness, `hello` has 24 commits both ways and its HEAD tree matches the source subtree. The same split commits feed all three layouts.

**Layouts.**
- **inline**: the full clone, plus stored splits as `refs/newgit/split/<shard>/<name>`.
- **separate**: 22,287 bare repos, each fetched from the split commits, one pack each.
- **pooled**: one repo with `refs/trackers/<shard>/<name>/main`, 12,000 extra synthetic namespaces pointing at random package heads (ref-count scaling only), and a root history of gitlink pins at `refs/heads/root`. Built with both reftable and files ref backends.

**Serving.** I used a small Python wrapper around the real `git http-backend` (`gitsrv.py`). It counts request and response body bytes, and can hide refs per request and run the reachability check. The wire bytes are body bytes only, without HTTP headers. I also ran spawns through the real `newgit serve` (release build). I logged in with its dev secret, linked a throwaway project, and swapped each layout's repo in as the project's `root` tracker repo.

**Item 1.** Each rep uses a fresh directory (5 reps; 1–2 for the multi-minute partial clones). Time covers the clone or fetch plus checkout. I report `git log` afterwards separately. Inline stored-split, pooled and separate-by-fetch use the same steps (init, one-ref fetch, checkout). The first inline partial clone ran before `git gc`, with no bitmaps; the rest ran after.

**Item 2.** One landing is: read the old split head, write the new split commit (tree is the package's already-existing subtree), and move the ref. I ran 300 landings for each ref backend.

**Item 3.** Median of 3 runs for each of 24 packages: `hello`, the three highest-churn packages (`azure-cli` 713, `claude-code` 361, `zed-editor` 358 commits), and 20 random ones. For inline I swapped the commit-graph between Bloom and none.

**Item 4.** Each round is 100 agents. Every agent has a ready commit changing `package.nix` of a different package. They submit at the same instant, and the server takes up to a batch at a time. A batch's agents are acknowledged after the landing-log commit, so latency is the burst-to-ack time. There are 10 rounds per cell. Preparing the agents' commits (the push) is not timed. Three landing paths:
- **inline `merge`**: serial fold. For each agent, `git merge-tree --write-tree --merge-base=<base> <tip> <agent>` then `git commit-tree`. One `update-ref --stdin` per batch also moves each touched package's split ref.
- **inline `fast`**: the same result without a merge. It checks the agent's path is untouched since its base, splices the package subtree into the tip tree, and writes all objects with one `hash-object` per batch.
- **separate** moves each package's ref (`update-ref` in its own repo, 16 in parallel or 1).
- **pooled** does one `start/prepare/commit` transaction per batch with every package head plus root's new head. Root gets one commit per landing, pinning by gitlink.
- Every layout also writes one landing-log commit per batch, as asked. Pooled doesn't strictly need it, since the transaction and root commit already are the log. That would save about 3 spawns per batch.

**Item 5.** The change appends a line to `package.nix` in 5,000 random packages. A real `sed` over a checkout was not used. Inline lands it as one landing: a merge-tree when main moved, the landing commit, 5,000 split commits, and one transaction moving 5,001 refs. Pooled writes the new objects into the one store, builds the root pin commit, and runs one transaction. Separate is S1 (commits already pushed into the 5,000 repos: 5,000 `update-ref`s) and S2 (the server writes blob, tree and commit into each repo and moves its ref). Inline's change commit was built before timing, like a push, so inline is closer to S1 and pooled's "landing only".

**Item 6.** `du` (blocks) plus apparent size, file counts from `find`, and object and ref counts from `count-objects` and `for-each-ref`. gc timings are `git gc` (bitmaps and commit-graph on in a bare repo). Inline was measured before and after gc: pre-gc it was 3.77 GB with 2 packs and 9.03M objects, because fetching the splits duplicated about 466k objects. A server that writes split commits directly would not have them; gc deduplicated.

**Item 7.** Rough prototype, in `views_bench.py`. Hidden packages are removed from each commit's tree bottom-up, memoized by source tree id, so a landing only re-filters trees along changed paths. Commits that change nothing visible are dropped. Parents are the previous kept commit. Objects are written by real git. It runs over a window of 3,000 first-parent commits, then 300 more one at a time (each flushed, which dominates the 23–32 ms). For pooled it first converts the content trees to pin trees (gitlinks) and then filters. That conversion is an artifact (a real pooled root already stores pins), so the pooled numbers are an upper bound. Separate adds a measured cost of resolving a package's subtree from its own repo.

**Reachability check.** `reach_check.py` times the `rev-list` walk directly, for reader sets of 1, 1k, about 20k and all 34k refs, for tip and blob wants, with and without bitmaps. `reach_e2e.py` times full fetches through the wrapper. `have` lines were not checked or measured.

## Caveats

- **Spawn-bound.** Every number except batched landings is dominated by process spawns, about 5–8 ms each on this Mac. A real server using in-process Git would be faster for single landings. Relative comparisons hold; absolute landing numbers shouldn't be quoted as server capacity.
- **Landing simulation is conflict-free.** Agents touch different packages, so conflicts, retries and the "barrier" paths in `home-design.md` were not exercised. There is no agent push transfer.
- **History is post-move only.** Per-package history starts when the package moved to `by-name` (about 2023–24). `git log --follow` across moves was not modelled. Real homes of this size will usually have longer histories, which makes stored splits and per-package refs more valuable and bare log/blame slower.
- **Whole nixpkgs vs by-name.** Inline holds all of nixpkgs. Separate and pooled hold only `by-name`. The non-package content (lib, nixos, old `pkgs/`) would live in root as ordinary history in a real home, adding roughly the 3 GB the inline repo carries.
- **Pooled ref count is partly synthetic.** 12,000 of its 34,288 namespaces are fake, added to reach the 30k+ you asked about. They affect ref counts, creation time, advertisement size and gc, but not histories.
- **Views are a prototype.** It ignores merges (first-parent only), commit messages that name hidden packages, and the real compose and split engine. The first-build figures over the whole by-name era are extrapolated from 3,000 commits. Private audiences in a real home must live in their own repos or pools, so these numbers cover only the cost of composing.
- **Other things not run or not measured:**
  - `git subtree split` over the full history (too slow; the custom tool was used instead).
  - Many simultaneous clones.
  - Disk-level isolation, backups and replication.
  - `newgit serve`'s landing API. It does not apply at this scale today, because it works on its own per-tracker model. Only its Git HTTP path was measured.
- **Wire bytes are loopback body bytes.** They exclude TLS and HTTP headers.
- **Noise.** Spawn timings of 0.09–0.18 s varied by about ±20 ms between runs and after gc. The difference between separate's clone (0.09–0.12 s) and the one-ref fetch (0.13 s) is mostly the extra `init` and `checkout`.

## Commands and scripts

Everything is under `~/newgit-bench/` (data in `np/`, scripts in `scripts/`, raw outputs in `results/`). Shared paths are in `scripts/env.sh`. I did not touch the pre-existing older directories in `~/newgit-bench/`.

```
git clone --bare --single-branch --branch master https://github.com/NixOS/nixpkgs np/src.git
git -C np/src.git commit-graph write --reachable --no-changed-paths      # then --changed-paths; graph.sh swaps them
git -C np/src.git ls-tree -r -d --name-only HEAD pkgs/by-name/ | awk -F/ 'NF==4' > np/packages.all.txt

scripts/build_separate.sh np/packages.{1k,5k,all}.txt np/sep-{1k,5k,all}   # calls split_pkgs.py; leaves np/scratch.git
scripts/make_inline.sh                                                      # np/inline-full.git + stored splits
python3 scripts/make_pooled.py reftable 12000                               # np/pooled-reftable.git
python3 scripts/make_pooled.py files 12000                                  # np/pooled-files.git
git clone --bare --shared --ref-format={files,reftable} np/src.git np/inline-{files,reftable}.git
git -C np/inline-X.git fetch np/scratch.git 'refs/split/*:refs/newgit/split/*'   # landing-test repos

python3 scripts/gitsrv.py np/srv 8100                       # git http-backend + byte counting (+ hideRefs, reachability check)
python3 scripts/spawn_bench.py pkgs/by-name/he/hello URL/nixpkgs.git URL/sep/he/hello.git 5 DIR \
    separate separate_fetch split pooled pooled_hidden pooled_reach sparse_blobnone sparse_blobnone_backfill sparse_tree0
python3 scripts/logblame_bench.py 3                         # item 3
python3 scripts/split_incr.py np/inline-X.git np/packages.all.txt 300 SEED   # item 2
scripts/run_landings.sh                                     # item 4 matrix (land_bench.py; gitlib.py helpers)
python3 scripts/tree_wide.py {inline|separate|pooled} REPO np/packages.all.txt 5000 SEED [--moved]   # item 5
python3 scripts/disk_stats.py LABEL PATH [--many]; scripts/gc_bench.sh   # item 6
python3 scripts/views_bench.py SRC VIEWREPO LIST N 200 3000 300 5 [SEP_ROOT]; PINS=1 ...             # item 7
python3 scripts/reach_check.py np/pooled-reftable.git 5; python3 scripts/reach_e2e.py 7              # reachability
python3 scripts/touched_stats.py                             # real landing sizes
scripts/serve_setup.sh                                       # real `newgit serve` (release build of `crates/newgit`)
```

Local data is about 8 GB (`np/src.git`, `inline-full.git`, `sep-all`, `pooled-*`, `scratch.git`, the two landing-test repos). `rm -rf ~/newgit-bench/np` removes it. I deleted the `cargo` target dir and temp clones.
