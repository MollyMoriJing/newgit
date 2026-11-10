import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WalkedCommit } from "../src/endpoint";
import { PULLS_ASKED, PULL_RETRY_MAX_MS, beyondMain } from "../src/webhook";
import {
	THEIRS,
	advance,
	asked,
	call,
	deliver,
	index,
	jack,
	main,
	path,
	plans,
	push,
	setAnswers,
	setGitHub,
	setLooks,
	sha,
	stub,
} from "./webhook-fixture";

// Every commit a pull would land beyond `main` is checked for an instance's
// withheld work, whichever event's payload it came in (#399). A commit
// refused once stays on the mirror, and the next event's pull lands it too:
// asking GitHub about only that event's commits let it through unasked. New
// tests live in their own file per AGENTS.md's "Working in parallel".

const REFUSED = sha("c");
const pullRequest = (ref: string) => ({ head: { ref, repo: { full_name: "Spheroman/newgit" } } });
const holding = (instance: string) => ({ name: instance, pins: { t3code: sha("7") } });
const landed = { outcome: "landed", list: { ...main, root: sha("f") }, commits: 2 } as const;
const refusal = { outcome: "refused", why: "run `newgit merge feature`, then redeliver this event" } as const;
const state = async () => (await call(jack, "GET", path("/state"))).body;
const walked = (commit: string, parents: string[], subject = ""): WalkedCommit => ({ commit, parents, subject });

async function instanceWith(instance: string) {
	await call(jack, "POST", path("/instances"), { name: instance, base_rev: "x" });
	await stub().startFork(instance, "t3code");
	await stub().completeFork(instance, "t3code");
	await call(jack, "PUT", path(`/checkpoints/${instance}/one`), { record: { code: { head: { t3code: sha("7") } } } });
}

let quiet: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
	quiet = vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
	quiet.mockRestore();
});

describe("what a pull would land beyond `main`", () => {
	it("asks about every commit no merge commit accounts for", () => {
		const walk = [walked(REFUSED, [main.root], "Feature (#7)"), walked(THEIRS, [REFUSED], "A change (#8)")];
		expect(beyondMain(walk)).toEqual({ branches: [], commits: [REFUSED, THEIRS], count: 2 });
	});

	it("takes a merge commit's subject for itself and what it merged, not for what came before", () => {
		const walk = [
			walked(sha("1"), [main.root], "a direct push"),
			walked(sha("2"), [main.root], "on the branch"),
			walked(sha("3"), [sha("2")], "on the branch too"),
			walked(sha("4"), [sha("1"), sha("3")], "Merge pull request #7 from Spheroman/feature"),
			walked(THEIRS, [sha("4")], "later"),
		];
		expect(beyondMain(walk)).toEqual({ branches: ["feature"], commits: [sha("1"), THEIRS], count: 2 });
	});

	it("keeps no more than it can ask about, and says how many there were", () => {
		const walk = Array.from({ length: 100 }, (_, n) => walked(n.toString(16).padStart(40, "0"), []));
		const beyond = beyondMain(walk);
		expect(beyond.count).toBe(100);
		expect(beyond.commits).toHaveLength(PULLS_ASKED + 1);
	});
});

describe("a commit refused, then carried by a later event", () => {
	it("is asked about again, and refused again, though the later event never named it", async () => {
		await instanceWith("feature");
		setGitHub((commit) => Response.json([pullRequest(commit === REFUSED ? "feature" : "other")]));
		setAnswers([refusal]);
		await deliver({ payload: push({ after: REFUSED, commits: [{ id: REFUSED }] }) });
		expect(plans[0].instances).toEqual([holding("feature")]);
		expect(await index()).toEqual(main);

		// The next event pushed only its own commit; the look says both lie
		// beyond `main`, and GitHub is asked about both.
		setLooks([{ outcome: "work", commits: [walked(REFUSED, [main.root], "Feature (#7)"), walked(THEIRS, [REFUSED])] }]);
		setAnswers([refusal]);
		asked.length = 0;
		await deliver();
		expect([...asked].sort()).toEqual([REFUSED, THEIRS].sort());
		expect(plans[1].instances, "the instance the earlier commit came from").toEqual([holding("feature")]);
		expect((await state()).ended.at(-1)).toMatchObject({ commit: THEIRS, outcome: "refused" });
		expect(await index()).toEqual(main);
	});

	it("is refused again when an earlier merge commit's subject names the branch, at no call for it", async () => {
		await instanceWith("feature");
		setGitHub(() => Response.json([pullRequest("other")]));
		setLooks([
			{
				outcome: "work",
				commits: [
					walked(sha("2"), [main.root], "on the branch"),
					walked(REFUSED, [main.root, sha("2")], "Merge pull request #7 from Spheroman/feature"),
					walked(THEIRS, [REFUSED], "A change (#8)"),
				],
			},
		]);
		setAnswers([refusal]);
		await deliver();
		expect(asked, "only the commit no merge accounts for").toEqual([THEIRS]);
		expect(plans[0].instances).toEqual([holding("feature")]);
	});

	it("is not refused forever: nothing remembers the refusal, and once the work is on `main` it lands", async () => {
		// The container decides from the pins: after `newgit merge feature`,
		// `feature`'s are on `main` and it lands what it refused before
		// (crates/newgit-core/tests/endpoint_pull_walk.rs).
		await instanceWith("feature");
		setGitHub((commit) => Response.json([pullRequest(commit === REFUSED ? "feature" : "other")]));
		setAnswers([refusal]);
		await deliver({ payload: push({ after: REFUSED, commits: [{ id: REFUSED }] }) });
		setLooks([{ outcome: "work", commits: [walked(REFUSED, [main.root]), walked(THEIRS, [REFUSED])] }]);
		setAnswers([landed]);
		await deliver();
		expect(plans[1].instances).toEqual([holding("feature")]);
		expect((await index()).root).toBe(sha("f"));
		expect((await state()).ended.at(-1)).toMatchObject({ commit: THEIRS, outcome: "landed" });
	});
});

describe("a walk too long to ask about", () => {
	it("gives up at once, blaming nobody, while some instance holds withheld work", async () => {
		await instanceWith("feature");
		const commits = Array.from({ length: PULLS_ASKED + 1 }, (_, n) => walked(n.toString(16).padStart(40, "0"), []));
		setLooks([{ outcome: "work", commits }]);
		await deliver();
		expect(asked).toEqual([]);
		expect(plans).toEqual([]);
		const ended = (await state()).ended.at(-1);
		expect(ended).toMatchObject({ outcome: "gave up" });
		expect(ended.why).toContain(`at most ${PULLS_ASKED}`);
		expect(ended.why).not.toContain("feature");
	});

	it("lands without a call when no instance holds withheld work", async () => {
		const commits = Array.from({ length: 100 }, (_, n) => walked(n.toString(16).padStart(40, "0"), []));
		setLooks([{ outcome: "work", commits }]);
		setAnswers([landed]);
		await deliver();
		expect(asked).toEqual([]);
		expect(plans[0].instances).toEqual([]);
		expect((await index()).root).toBe(sha("f"));
	});
});

describe("a look that does not say what lies beyond `main`", () => {
	beforeEach(() => {
		vi.useFakeTimers({ toFake: ["Date"] });
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("lands nothing unasked: a container older than the Worker is a failed attempt", async () => {
		await instanceWith("feature");
		setLooks([{ outcome: "work" } as never]);
		setAnswers([landed]);
		await deliver();
		expect(plans).toEqual([]);
		const [waiting] = await stub().pulls();
		expect(waiting.attempts).toBe(1);
		expect(waiting.lastError).toContain("without the commits it would land");
		expect(waiting.beyond, "it looks again").toBeUndefined();

		vi.setSystemTime(Date.now() + PULL_RETRY_MAX_MS);
		setGitHub(() => Response.json([pullRequest("other")]));
		await advance();
		expect(plans[0].instances).toEqual([]);
		expect((await index()).root).toBe(sha("f"));
	});
});
