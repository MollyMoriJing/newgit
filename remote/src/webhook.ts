// GitHub's push webhook, which brings merged pull requests into newgit.dev
// (docs/design/remote.md, *The GitHub webhook*). GitHub expects a quick
// answer and the work takes as long as the container does, so the route only
// verifies the event and queues it on the project; `ProjectDO`'s alarm then
// moves the queue on with `advancePulls`, which is also what keeps the
// endpoint awake while its pull runs: nobody else is polling it.
//
// A pull first looks, and ends there when there is nothing to land: every
// export of ours comes back as an event. Otherwise it lands as a merge does.
// The container pushes its commits to landing refs, `main`'s list moves by
// compare-and-swap, and the repos' branches follow. A merge that landed
// meanwhile sends it back to start from `main` as it is. Every step can run
// twice: a redelivery, an evicted object, or a retry finds the work done.

import {
	type Deps,
	type ExportDeclaration,
	HttpError,
	isChecking,
	anchorFor,
	landAndFollow,
	loadDefinitions,
	makeTag,
	projectStub,
	repoNamer,
	revokeRepoTokens,
	settle,
	type TrackerFacts,
} from "./index";
import { pausedBy } from "./budget";
import type { MirrorInstance, PullPlan, WalkedCommit } from "./endpoint";
import { decide, mayMaintain, type ProjectId, projectKey, repoName } from "./names";

/** Who a pull lands as, in `main`'s log. Not a login: logins have no spaces. */
export const WEBHOOK_MERGER = "the GitHub webhook";

/** The branch a mirror's main branch stands for, as an export pushes it. */
export const PULL_BRANCH = "main";

/** How often the alarm looks at a pull: a poll of the container. */
export const PULL_POLL_MS = 15_000;

/** How soon after an event the alarm first runs: GitHub has had its answer. */
export const PULL_START_MS = 1_000;

/** Past this, a pull still not landed gives up; a redelivery starts it again. */
export const PULL_LIMIT_MS = 2 * 60 * 60_000;

/**
 * How many failed attempts a pull makes before it gives up. With the wait
 * between them doubling from `PULL_POLL_MS` to `PULL_RETRY_MAX_MS`, that is
 * about three quarters of an hour: an outage of GitHub's, or the container
 * dropping under a deploy, is over long before.
 */
export const PULL_ATTEMPTS = 10;

/** The longest wait between two attempts, or two failed lookups. */
export const PULL_RETRY_MAX_MS = 10 * 60_000;

/** How long after its `attempts`th failure a pull is tried again. */
export function retryDelay(attempts: number, first = PULL_POLL_MS): number {
	return Math.min(first * 2 ** Math.max(attempts - 1, 0), PULL_RETRY_MAX_MS);
}

/** The passwords a pull's container fetches and pushes with: past `CHECK_LIMIT_MS`. */
const PULL_CREDENTIAL_SECONDS = 50 * 60;

/** One verified event, waiting for or being worked on by the container. */
export interface MirrorPull {
	target: string;
	/** `owner/name` on GitHub, as the signed event gives it. */
	repo: string;
	branch: string;
	commit: string;
	/** Who pushed, by GitHub login: the login newgit knows them by. */
	pusher: string;
	/** The trackers this pull may change: what the pusher maintains. */
	writable: string[];
	/** The trackers the mirror withholds. */
	withheld: string[];
	/** Instances GitHub says this came from that hold withheld work, known once it has said. */
	instances?: MirrorInstance[];
	/**
	 * Asking GitHub which pull request this came from: when it was last asked,
	 * since when it has listed none, and why it could not answer, how many
	 * times in a row. Until it answers, the pull waits.
	 */
	lookup?: { since?: number; last: number; failed?: string; failures?: number };
	queuedAt: number;
	attempts: number;
	/** Why the last attempt failed, for `newgit status` to say. */
	lastError?: string;
	/** After a failure, not tried again before this. */
	retryAt?: number;
	/** What a look found this pull would land beyond `main`; unset until one has. */
	beyond?: Beyond;
	/** Read passwords handed out for looks, to cancel when the pull ends. */
	tokens?: { repo: string; artifactsId: string }[];
	/** `main`'s index when this pull began to land; unset until then. */
	base?: Record<string, string>;
	/** A tag made on the mirror: `commit` is its object, and nothing lands but the tag. */
	tag?: string;
}

/**
 * Whether `header` is GitHub's signature of `body`: `sha256=` and the
 * HMAC-SHA256 of the raw bytes. `verify` compares in constant time.
 */
export async function verifySignature(secret: string, body: ArrayBuffer, header: string | null): Promise<boolean> {
	const hex = header?.match(/^sha256=([0-9a-f]{64})$/)?.[1];
	if (hex === undefined) return false;
	const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, [
		"verify",
	]);
	const signature = Uint8Array.from(hex.match(/../g) ?? [], (pair) => Number.parseInt(pair, 16));
	return crypto.subtle.verify("HMAC", key, signature, body);
}

/** The `owner/name` an export target's `url` is on GitHub, lowercased; null when it is elsewhere. */
export function githubRepo(url: string): string | null {
	const found =
		url.match(/^(?:https:\/\/)?github\.com\/([\w.-]+\/[\w.-]+?)(?:\.git)?\/?$/) ??
		url.match(/^git@github\.com:([\w.-]+\/[\w.-]+?)(?:\.git)?$/);
	return found ? found[1].toLowerCase() : null;
}

/** A target's name, as the client gives it: its `name`, or its repository's. */
export function targetName(declaration: ExportDeclaration): string {
	if (declaration.name !== undefined) return declaration.name;
	const last = declaration.url.replace(/\/+$/, "").split(/[/:]/).pop() ?? "";
	return last.endsWith(".git") ? last.slice(0, -".git".length) : last;
}

/** The tracker whose export `"inherit"` takes: `t3code` for `t3code__repos`, root for the rest. */
function exportParent(tracker: string): string | null {
	if (tracker === "root") return null;
	const at = tracker.lastIndexOf("__");
	return at < 0 ? "root" : tracker.slice(0, at);
}

/**
 * Whether `tracker` ships in the target `declaredBy` declares: it is that
 * tracker, or it says `export = "inherit"` all the way up to it. As the
 * client decides it (`export_target::resolve_targets`).
 */
export function shipsIn(tracker: string, declaredBy: string, facts: Map<string, TrackerFacts>): boolean {
	let at: string | null = tracker;
	while (at !== null) {
		if (at === declaredBy) return facts.get(at)?.git === true;
		const found = facts.get(at);
		if (found?.git !== true || !(found.export ?? []).includes("inherit")) return false;
		at = exportParent(at);
	}
	return false;
}

/** The target a GitHub repo is, among the definitions: the one declaring a URL there. */
export function targetFor(
	repo: string,
	facts: Map<string, TrackerFacts>,
): { declaredBy: string; declaration: ExportDeclaration } | null {
	for (const [tracker, found] of facts) {
		if (!found.git) continue;
		for (const entry of found.export ?? []) {
			if (entry !== "inherit" && githubRepo(entry.url) === repo.toLowerCase()) return { declaredBy: tracker, declaration: entry };
		}
	}
	return null;
}

/** The slug newgit names an instance by: its branch, as `branch_slug` makes it. */
function instanceSlug(branch: string): string {
	return branch
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-+|-+$/g, "");
}

/** The most commits one event asks GitHub about; past it, nobody can say. */
export const PULLS_ASKED = 20;

/**
 * How long "no pull request" is not believed. GitHub's list can lag the push
 * that told us, and believing it too soon lands a squash without the withheld
 * work of the instance it came from. Past this, it is a direct push.
 */
export const PULL_INDEX_WAIT_MS = 2 * 60_000;

/** How often GitHub is asked again during that wait: not every poll. */
export const PULL_INDEX_ASK_MS = 30_000;

/** How long GitHub gets to say which pull request a commit came from. */
const GITHUB_TIMEOUT_MS = 10_000;

/** The branch GitHub's merge commit subject names, if it is one. */
function mergedBranch(subject: string): string | undefined {
	return subject.match(/^Merge pull request #\d+ from [^/\s]+\/(\S+)/)?.[1];
}

/**
 * What a pull would land beyond `main`, as far as whose work it is goes: the
 * branches GitHub's merge commits there name, and the commits no such merge
 * brought in, which GitHub is asked about. Every one of them, whichever
 * event's payload it came in (#399): a commit refused once stays on the
 * mirror, and the next event's pull would otherwise land it unasked. `count`
 * is how many commits there are to ask about; past `PULLS_ASKED`, only that
 * many are kept.
 */
export interface Beyond {
	branches: string[];
	commits: string[];
	count: number;
}

/**
 * `walk`, as a look reports it, reduced to what says whose work it is. A
 * merge commit whose subject names a branch speaks for itself and for what
 * it merged: the commits its other parents reach that its first does not.
 */
export function beyondMain(walk: WalkedCommit[]): Beyond {
	const parents = new Map(walk.map((walked) => [walked.commit, walked.parents]));
	const reach = (from: string[]) => {
		const seen = new Set<string>();
		const stack = [...from];
		for (let commit = stack.pop(); commit !== undefined; commit = stack.pop()) {
			if (seen.has(commit) || !parents.has(commit)) continue;
			seen.add(commit);
			stack.push(...(parents.get(commit) ?? []));
		}
		return seen;
	};
	const branches = new Set<string>();
	const accounted = new Set<string>();
	for (const walked of walk) {
		const named = mergedBranch(walked.subject);
		if (named === undefined) continue;
		branches.add(named);
		accounted.add(walked.commit);
		const before = reach(walked.parents.slice(0, 1));
		for (const merged of reach(walked.parents.slice(1))) if (!before.has(merged)) accounted.add(merged);
	}
	const asked = walk.map((walked) => walked.commit).filter((commit) => !accounted.has(commit));
	return { branches: [...branches], commits: asked.slice(0, PULLS_ASKED + 1), count: asked.length };
}

/**
 * The branches of the pull requests `commits` came from, as GitHub says. Any
 * pull request listed counts, marked merged or not: the push event can beat
 * that mark, and missing the pull request would land work without what it
 * holds. One from a fork does not: its branch names no instance here. Throws
 * when GitHub cannot answer.
 */
async function pullRequestBranches(
	fetcher: typeof fetch,
	repo: string,
	commits: string[],
): Promise<{ branches: string[]; unlisted: boolean }> {
	const asked = await Promise.all(
		commits.map(async (commit) => {
			const response = await fetcher(`https://api.github.com/repos/${repo}/commits/${commit}/pulls`, {
				headers: { accept: "application/vnd.github+json", "user-agent": "newgit-remote" },
				signal: AbortSignal.timeout(GITHUB_TIMEOUT_MS),
			});
			if (!response.ok) throw new Error(`GitHub answered ${response.status} for ${commit}`);
			return (await response.json()) as { head?: { ref?: string; repo?: { full_name?: string } | null } }[];
		}),
	);
	return {
		branches: asked
			.flat()
			.filter((pull) => pull.head?.repo?.full_name?.toLowerCase() === repo.toLowerCase())
			.flatMap((pull) => pull.head?.ref ?? []),
		unlisted: asked.some((pulls) => pulls.length === 0),
	};
}

/** What GitHub said of whose work a pull's commits are. */
export type Implicated =
	| { instances: MirrorInstance[]; unlisted: boolean }
	/** GitHub could not say. `retry` is false when asking again cannot help. */
	| { unknown: string; retry: boolean };

/**
 * The instances among `work` whose withheld work this pull would land: what
 * it brings in `beyond` `main`. A mirror's branch is named for its instance,
 * and a push to `main` does not say which branch it merged, so GitHub's merge
 * subjects say, or else GitHub.
 * Only an instance GitHub names counts: one whose branch is not on GitHub
 * yet, or that merely has a fork, is no part of this. `unlisted` says GitHub
 * listed nothing for some commit, which may be a direct push, or may be
 * GitHub not having caught up: the caller decides how long to wait. When
 * GitHub cannot answer, nobody is named and the caller waits for it.
 */
export async function implicated(
	deps: Deps,
	pull: Pick<MirrorPull, "repo" | "commit">,
	beyond: Beyond,
	work: MirrorInstance[],
): Promise<Implicated> {
	if (work.length === 0) return { instances: [], unlisted: false };
	if (beyond.count > PULLS_ASKED)
		return {
			unknown: `it would land ${beyond.count} commits beyond \`${PULL_BRANCH}\` that no merge commit accounts for, and GitHub is asked about at most ${PULLS_ASKED}`,
			retry: false,
		};
	let branches = beyond.branches;
	let unlisted = false;
	if (beyond.commits.length > 0) {
		try {
			const asked = await pullRequestBranches(deps.fetcher, pull.repo, beyond.commits);
			branches = [...branches, ...asked.branches];
			unlisted = asked.unlisted;
		} catch (error) {
			const why = error instanceof Error ? error.message : String(error);
			console.error(JSON.stringify({ webhook: "GitHub could not say which pull request this is", repo: pull.repo, commit: pull.commit, why }));
			return { unknown: why, retry: true };
		}
	}
	const slugs = new Set(branches.map(instanceSlug));
	return { instances: work.filter((instance) => slugs.has(instance.name)), unlisted };
}

/** What to do about a pull that ended without landing, for its `why`. */
function bringItIn(target: string): string {
	return `redeliver the event from GitHub's *Recent Deliveries*, or run \`newgit pull --remote ${target}\` from the project root`;
}

const refused = (status: number, reason: string, context: Record<string, unknown>): never => {
	console.error(JSON.stringify({ webhook: "refused", reason, ...context }));
	throw new HttpError(status, reason);
};

/**
 * `POST /v1/webhooks/github?project=<owner>/<name>`. Which project an event
 * is for is in the URL, set when the webhook was made; one of the project's
 * tracker definitions on `main` then has to declare an `export` to the repo
 * the event came from, so one repo's event cannot move a project that does
 * not say it mirrors there. That declaration is the target: a repo is one.
 */
export async function githubWebhook(request: Request, env: Env, deps: Deps): Promise<Response> {
	if (!env.GITHUB_WEBHOOK_SECRET) throw new HttpError(503, "this remote has no GITHUB_WEBHOOK_SECRET");
	const raw = await request.arrayBuffer();
	if (!(await verifySignature(env.GITHUB_WEBHOOK_SECRET, raw, request.headers.get("x-hub-signature-256"))))
		throw new HttpError(401, "bad signature");

	const event = request.headers.get("x-github-event");
	if (event === "ping") return Response.json({ pong: true });
	if (event !== "push") return Response.json({ ignored: `${event} events are not used` });

	let payload: {
		ref?: string;
		after?: string;
		deleted?: boolean;
		repository?: { full_name?: string; private?: boolean };
		pusher?: { name?: string };
	};
	try {
		payload = JSON.parse(new TextDecoder().decode(raw));
	} catch {
		throw new HttpError(400, "the event is not JSON");
	}
	const url = new URL(request.url);
	const repo = payload.repository?.full_name ?? "";
	const context = { project: url.searchParams.get("project"), repo };
	const id = parseProject(context.project);
	if (id === null) return refused(400, "the webhook's URL needs `?project=<owner>/<name>`", context);
	const project = projectStub(env, id);
	const info = await project.info();
	if (info === null) return refused(404, `no project \`${projectKey(id)}\``, context);

	const list = (await project.branch(PULL_BRANCH))?.list;
	if (list === undefined) return refused(404, `no branch \`${PULL_BRANCH}\``, context);
	const facts = await loadDefinitions(deps.repos, id, Object.keys(list));
	const found = targetFor(repo, facts);
	if (found === null) return refused(403, `no tracker on \`${PULL_BRANCH}\` exports to ${repo}`, context);
	const target = targetName(found.declaration);
	const branch = found.declaration.branch ?? "main";

	if (found.declaredBy !== "root")
		return Response.json({ ignored: "a rooted target's commits are its tracker's: `newgit pull --remote`" });
	const tag = payload.ref?.startsWith("refs/tags/") ? payload.ref.slice("refs/tags/".length) : undefined;
	if (tag !== undefined && payload.deleted === true)
		return Response.json({ ignored: "a tag is deleted on the remote, not by a mirror: tags never move" });
	if (tag === undefined && payload.ref !== `refs/heads/${branch}`)
		return Response.json({ ignored: `not \`${branch}\`` });
	const commit = payload.after ?? "";
	if (payload.deleted === true || !/^[0-9a-f]{40}$/.test(commit) || /^0+$/.test(commit))
		return Response.json({ ignored: "the branch was deleted" });
	if (payload.repository?.private === true)
		return refused(422, "a private mirror cannot be read: the endpoint fetches mirrors without credentials", context);
	if (await pausedBy(env)) return refused(503, "this remote is paused", context);

	// Landing here is a merge by whoever pushed there: they must be able to
	// merge here, and what they do not maintain stays as it is.
	const pusher = (payload.pusher?.name ?? "").toLowerCase();
	const who = { login: pusher, member: await project.isMember(pusher) };
	const ships = (tracker: string) => shipsIn(tracker, found.declaredBy, facts);
	const writable = Object.keys(list).filter((tracker) => {
		const found = facts.get(tracker);
		return (
			found !== undefined &&
			ships(tracker) &&
			mayMaintain(found.maintainers, who, info.owner, tracker).ok &&
			decide({ kind: "tracker", project: id, tracker }, "read", who, { tracker: found }).ok
		);
	});
	const withheld = Object.keys(list).filter((tracker) => !ships(tracker));
	if (!writable.includes("root"))
		return refused(403, `\`${pusher}\` pushed this, and may not merge into ${projectKey(id)}`, context);

	// Queued behind any pull, so a pull request merged and then tagged
	// there is on `main` by the time its tag is looked for.
	await project.queuePull({
		target,
		repo,
		branch,
		commit,
		pusher,
		writable,
		withheld,
		queuedAt: Date.now(),
		attempts: 0,
		...(tag === undefined ? {} : { tag }),
	});
	return Response.json({ queued: commit }, { status: 202 });
}

function parseProject(key: string | null) {
	if (key === null) return null;
	const [owner, name, ...rest] = key.split("/");
	return owner && name && rest.length === 0 ? { owner: owner.toLowerCase(), name } : null;
}

/**
 * Move the oldest pull on. True when there is more to do. Only `ProjectDO`'s
 * alarm calls this, so two steps never run at once. A failure is logged and
 * retried, each time a little later; past `PULL_ATTEMPTS`, or `PULL_LIMIT_MS`,
 * it gives up. Every pull that ends says how where `newgit status` reads it,
 * a give-up with what to do, so none ends without a word.
 */
export async function advancePulls(env: Env, deps: Deps, id: ProjectId): Promise<boolean> {
	const project = projectStub(env, id);
	const [pull] = await project.pulls();
	if (pull === undefined) return false;
	const log = { project: projectKey(id), target: pull.target, commit: pull.commit };
	const now = Date.now();
	// Waiting out its last failure: the queue waits with it, in order.
	if (pull.retryAt !== undefined && now < pull.retryAt && now - pull.queuedAt <= PULL_LIMIT_MS) return true;
	let done = false;
	let gaveUp: string | undefined;
	try {
		done = await advance(env, deps, id, pull);
	} catch (error) {
		const why = error instanceof Error ? error.message : String(error);
		if (error instanceof HttpError && error.status === 409) {
			// `main` is not what this pull began from: the same base would
			// only fail again, so it starts from what `main` is now.
			console.log(JSON.stringify({ webhook: "starting again from `main`", ...log, why }));
			await project.restartPull(pull);
		} else {
			const attempts = await project.pullFailed(pull, why, Date.now() + retryDelay(pull.attempts + 1));
			done = attempts >= PULL_ATTEMPTS;
			if (done) gaveUp = `it failed ${attempts} times; the last: ${why}`;
			console.error(JSON.stringify({ webhook: "pull failed", ...log, why, attempts, givingUp: done }));
		}
	}
	if (!done && Date.now() - pull.queuedAt > PULL_LIMIT_MS) {
		const last = (await project.pulls()).find((known) => known.target === pull.target && known.commit === pull.commit);
		gaveUp = `it waited ${PULL_LIMIT_MS / 3_600_000} hours, ${
			last?.lookup?.failed !== undefined && last.instances === undefined
				? `and GitHub still could not say which pull request it came from (${last.lookup.failed})`
				: last?.lastError !== undefined
					? `failing ${last.attempts} times; the last: ${last.lastError}`
					: "without finishing"
		}`;
		done = true;
	}
	if (gaveUp !== undefined) {
		console.error(JSON.stringify({ webhook: "pull gave up", ...log, why: gaveUp }));
		await project.notePullEnded({
			target: pull.target,
			commit: pull.commit,
			outcome: "gave up",
			why: `${gaveUp}. Nothing moved: ${bringItIn(pull.target)}`,
			at: new Date().toISOString(),
		});
	}
	if (done) await revokeRepoTokens(deps, await project.dropPull(pull));
	return (await project.pulls()).length > 0;
}

/** One step of `pull`. True when it is finished, whatever came of it. */
async function advance(env: Env, deps: Deps, id: ProjectId, pull: MirrorPull): Promise<boolean> {
	const key = projectKey(id);
	const project = projectStub(env, id);
	const namer = await repoNamer(project, id);
	const log = { project: key, target: pull.target, commit: pull.commit, pusher: pull.pusher };
	let instances = pull.instances;
	const plan = (list: Record<string, string>): PullPlan => ({
		dir: "/work",
		repos: Object.fromEntries(
			trackersOf(list).map((tracker) => [
				tracker,
				{ url: deps.repos.url(namer(tracker)), auth: "" },
			]),
		),
		list,
		writable: pull.writable,
		target: pull.target,
		mirror: { url: `https://github.com/${pull.repo}.git`, branch: pull.branch, commit: pull.commit },
		instances: instances ?? [],
	});
	const mint = async (list: Record<string, string>, scope: (tracker: string) => "read" | "write") => {
		const handed = [];
		const minted: PullPlan["repos"] = {};
		for (const tracker of trackersOf(list)) {
			const name = namer(tracker);
			const token = await deps.repos.mint(name, scope(tracker), PULL_CREDENTIAL_SECONDS);
			handed.push({ repo: name, artifactsId: token.id });
			minted[tracker] = { url: deps.repos.url(name), auth: `Authorization: Basic ${btoa(`x:${token.password}`)}` };
		}
		return { handed, minted };
	};
	const ended = async (
		answer: { outcome: "nothing" } | { outcome: "refused"; why: string } | { outcome: "gave up"; why: string },
	) => {
		if (answer.outcome === "nothing") console.log(JSON.stringify({ webhook: "nothing to bring in", ...log }));
		else console.error(JSON.stringify({ webhook: answer.outcome, ...log, why: answer.why }));
		// Said where `newgit status` reads it: a refusal is the pusher's to
		// fix, and "nothing" says the mirror's commit is on `main`, which
		// ends whatever trouble an earlier pull from there had.
		await project.notePullEnded({
			target: pull.target,
			commit: pull.commit,
			outcome: answer.outcome,
			...(answer.outcome === "nothing" ? {} : { why: answer.why }),
			at: new Date().toISOString(),
		});
		return true;
	};
	if (!deps.pull) throw new HttpError(501, "this remote has no Git endpoint to pull with");
	// A pull looks at, and lands on, `main` as a read sees it: whatever
	// landed below root rolled up first.
	// One that cannot counts as a failed attempt, retried and then given up.
	if (!(await settle(deps, project, id, PULL_BRANCH)))
		throw new Error(`root could not roll up on \`${PULL_BRANCH}\`; the pull waits for it`);

	// A tag lands nothing: the container says which root commit it names,
	// and the tag is made here, as a push from the project root makes one.
	// Our own export of a tag comes back as an event too, and finds it made.
	if (pull.tag !== undefined) {
		const list = (await project.branch(PULL_BRANCH))?.list;
		if (list === undefined) throw new HttpError(404, `no branch \`${PULL_BRANCH}\` in ${key}`);
		const found = await deps.pull(key, { ...plan(list), tag: pull.tag }, async () => {
			const { handed, minted } = await mint(list, () => "read");
			await project.notePullTokens(pull, handed);
			return minted;
		});
		if (isChecking(found)) return false;
		if (found.outcome === "refused") return await ended({ outcome: "refused", why: `tag \`${pull.tag}\`: ${found.why}` });
		if (found.outcome !== "tagged") throw new HttpError(500, `a tag's look answered \`${found.outcome}\``);
		try {
			const made = await makeTag(env, deps, null, project, id, pull.tag, found.root, found.annotation, pull.pusher);
			if (isChecking(made)) return false;
			console.log(JSON.stringify({ webhook: made.created ? "tagged" : "tag already there", ...log, tag: pull.tag, root: found.root }));
		} catch (error) {
			// A refusal is an answer: a tag that would move, or a commit not on `main`.
			if (!(error instanceof HttpError) || error.status >= 500) throw error;
			return await ended({ outcome: "refused", why: `tag \`${pull.tag}\`: ${error.message}` });
		}
		return true;
	}

	// First a look: every export comes back as an event, and most end here.
	// It also says everything this pull would land beyond `main`, which is
	// what GitHub is asked about below, not just what the event pushed.
	let beyond = pull.beyond;
	if (beyond === undefined) {
		const list = (await project.branch(PULL_BRANCH))?.list;
		if (list === undefined) throw new HttpError(404, `no branch \`${PULL_BRANCH}\` in ${key}`);
		const seen = await deps.pull(key, { ...plan(list), peek: true }, async () => {
			const { handed, minted } = await mint(list, () => "read");
			await project.notePullTokens(pull, handed);
			return minted;
		});
		if (isChecking(seen)) return false;
		if (seen.outcome === "nothing" || seen.outcome === "refused") return await ended(seen);
		// A container older than this Worker says nothing of the walk:
		// landing unasked is what #399 was, so that is a failed attempt.
		if (seen.outcome !== "work" || !Array.isArray(seen.commits))
			throw new Error(`a look answered \`${seen.outcome}\` without the commits it would land`);
		beyond = beyondMain(seen.commits);
		await project.markPeeked(pull, beyond);
	}

	// Asked only now: our own exports end above, and cost GitHub no call.
	if (instances === undefined) {
		const now = Date.now();
		const lookup = pull.lookup;
		if (lookup !== undefined) {
			const wait = lookup.failed !== undefined ? retryDelay(lookup.failures ?? 1, PULL_INDEX_ASK_MS) : PULL_INDEX_ASK_MS;
			if (now - lookup.last < wait) return false;
		}
		const found = await implicated(deps, pull, beyond, await project.withheldWork(pull.withheld));
		if ("unknown" in found) {
			// Not knowing is not a refusal: nobody is named, and the pull
			// waits for GitHub, asking again later each time, until
			// `PULL_LIMIT_MS` gives up saying so.
			if (!found.retry)
				return await ended({
					outcome: "gave up",
					why: `whose work commit ${pull.commit} holds cannot be known: ${found.unknown}. Nothing moved: run \`newgit pull --remote ${pull.target}\` from the project root`,
				});
			await project.noteLookup(pull, {
				...(lookup?.since === undefined ? {} : { since: lookup.since }),
				last: now,
				failed: found.unknown,
				failures: (lookup?.failures ?? 0) + 1,
			});
			return false;
		}
		const since = lookup?.since ?? now;
		if (found.unlisted && now - since < PULL_INDEX_WAIT_MS) {
			await project.noteLookup(pull, { since, last: now });
			return false;
		}
		instances = found.instances;
		await project.setPullInstances(pull, instances);
	}

	const begun = await project.beginPull(pull);
	if (!begun.ok) throw new HttpError(404, `no branch \`${PULL_BRANCH}\` in ${key}`);
	const answer = await deps.pull(key, plan(begun.base), async () => {
		const { handed, minted } = await mint(begun.base, (tracker) =>
			pull.writable.includes(tracker) ? "write" : "read",
		);
		await project.notePullTokens(pull, handed);
		return minted;
	});
	if (isChecking(answer)) return false;

	if (answer.outcome === "landed") {
		// The commit point: `main`'s list, by compare-and-swap. A merge that
		// landed meanwhile sends this pull back to start from it.
		// It lands as a merge does: at its anchor, the lowest tracker it
		// touches, so a change on the mirror to one tracker neither waits on
		// root nor sends a merge elsewhere back to start (its commits are at
		// their landing refs, so the anchor can be read).
		const moved = [...new Set([...Object.keys(begun.base), ...Object.keys(answer.list)])].filter(
			(tracker) => begun.base[tracker] !== answer.list[tracker],
		);
		const anchor = await anchorFor(deps, id, begun.base, answer.list, moved, project, PULL_BRANCH);
		const landed = await landAndFollow(
			deps,
			project,
			id,
			PULL_BRANCH,
			begun.base,
			answer.list,
			WEBHOOK_MERGER,
			answer.moves ?? [],
			true,
			anchor,
		);
		if (!landed.ok) throw new HttpError(409, `the pull could not land: \`${PULL_BRANCH}\` ${landed.reason}`);
		await project.notePullEnded({
			target: pull.target,
			commit: pull.commit,
			outcome: "landed",
			why: anchor === "root" ? undefined : `at \`${anchor.split("__").join("/")}\``,
			at: new Date().toISOString(),
		});
		console.log(
			JSON.stringify({ webhook: "landed", ...log, commits: answer.commits, root: answer.list.root, behind: landed.behind }),
		);
		return true;
	}
	if (answer.outcome === "work" || answer.outcome === "tagged")
		throw new HttpError(500, `a landing answered \`${answer.outcome}\``);
	return await ended(answer);
}

const trackersOf = (list: Record<string, string>) => [...new Set([...Object.keys(list), "access"])];
