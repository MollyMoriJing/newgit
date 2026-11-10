import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
	PULLS_ASKED,
	PULL_INDEX_ASK_MS,
	PULL_LIMIT_MS,
	PULL_POLL_MS,
	PULL_RETRY_MAX_MS,
	retryDelay,
} from "../src/webhook";
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
	peeks,
	plans,
	push,
	setAnswers,
	setGitHub,
	sha,
	stub,
} from "./webhook-fixture";

// When GitHub cannot say which pull request a commit came from (#392): on
// 2026-11-09 api.github.com was out of reach for half an hour, and every
// instance holding withheld work was counted, so two merges that could lose
// nothing were refused, naming an instance whose branch was not on GitHub
// yet. Not knowing is not a refusal: nobody is named, and the pull waits for
// GitHub. New tests live in their own file per AGENTS.md's "Working in
// parallel".

const merged = { ...main, root: sha("f") };
const landed = { outcome: "landed", list: merged, commits: 1 } as const;
const pullRequest = (ref: string) => ({ head: { ref, repo: { full_name: "Spheroman/newgit" } } });
const state = async () => (await call(jack, "GET", path("/state"))).body;

/** An instance with a fork of `t3code`, and a checkpoint saying it holds work there. */
async function instanceWith(instance: string) {
	await call(jack, "POST", path("/instances"), { name: instance, base_rev: "x" });
	await stub().startFork(instance, "t3code");
	await stub().completeFork(instance, "t3code");
	await call(jack, "PUT", path(`/checkpoints/${instance}/one`), { record: { code: { head: { t3code: sha("7") } } } });
}

const failing: [string, () => Response][] = [
	["a rate limit", () => new Response("rate limited", { status: 403 })],
	["a 5xx", () => new Response("down", { status: 503 })],
	["a commit GitHub does not know yet", () => new Response("not found", { status: 404 })],
	[
		"no connection",
		() => {
			throw new Error("dial tcp 140.82.116.6:443: i/o timeout");
		},
	],
];

let quiet: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
	vi.useFakeTimers({ toFake: ["Date"] });
	quiet = vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
	quiet.mockRestore();
	vi.useRealTimers();
});

/** Let `ms` pass, and run a step of the queue, as a poll would. */
const later = async (ms: number) => {
	vi.setSystemTime(Date.now() + ms);
	return advance();
};

describe("GitHub not saying which pull request a commit came from", () => {
	it.each(failing)("on %s, blames nobody, refuses nothing, and waits", async (_case, answer) => {
		await instanceWith("t3-f9e7e893");
		setGitHub(answer);
		await deliver();
		expect(plans, "nothing landed, and nothing was refused").toEqual([]);
		expect((await state()).ended, "it has not ended").toEqual([]);
		const [waiting] = await stub().pulls();
		expect(waiting.instances, "nobody is named").toBeUndefined();
		expect(waiting.attempts, "GitHub's trouble is not an attempt").toBe(0);
		expect(await index()).toEqual(main);
	});

	it("lands the merges of 2026-11-09 once GitHub answers, naming nobody unrelated", async () => {
		// #387 came from `t3/195bf85f`; `t3-f9e7e893` held withheld work, and
		// its branch was not on GitHub yet.
		await instanceWith("t3-f9e7e893");
		let reachable = false;
		setGitHub(() => {
			if (!reachable) throw new Error("dial tcp 140.82.116.6:443: i/o timeout");
			return Response.json([pullRequest("t3/195bf85f")]);
		});
		setAnswers([landed]);
		await deliver();
		for (let step = 0; step < 5; step++) await later(PULL_RETRY_MAX_MS);
		expect(plans).toEqual([]);
		reachable = true;
		await later(PULL_RETRY_MAX_MS);
		expect(plans[0].instances, "only what GitHub named counts").toEqual([]);
		expect((await index()).root).toBe(sha("f"));
		expect((await state()).ended.at(-1)).toMatchObject({ outcome: "landed" });
		expect(quiet.mock.calls.flat().join("\n")).not.toContain("newgit merge");
	});

	it("still refuses when GitHub, answering at last, names the instance", async () => {
		await instanceWith("feature");
		let reachable = false;
		setGitHub(() => (reachable ? Response.json([pullRequest("feature")]) : new Response("down", { status: 503 })));
		setAnswers([{ outcome: "refused", why: "commit came from `feature`'s branch" }]);
		await deliver();
		reachable = true;
		await later(PULL_RETRY_MAX_MS);
		expect(plans[0].instances).toEqual([{ name: "feature", pins: { t3code: sha("7") } }]);
		expect((await state()).ended.at(-1)).toMatchObject({ outcome: "refused" });
	});

	it("asks again later each time, and does not count a wait as a failure", async () => {
		await instanceWith("feature");
		setGitHub(() => new Response("down", { status: 503 }));
		await deliver();
		expect(asked).toHaveLength(1);
		await later(PULL_INDEX_ASK_MS - 1);
		expect(asked, "not before the first wait is over").toHaveLength(1);
		await later(1);
		expect(asked).toHaveLength(2);
		await later(PULL_INDEX_ASK_MS);
		expect(asked, "the second wait is longer").toHaveLength(2);
		await later(retryDelay(2, PULL_INDEX_ASK_MS) - PULL_INDEX_ASK_MS);
		expect(asked).toHaveLength(3);
		// Grown to the longest wait, it asks once in every `PULL_RETRY_MAX_MS`:
		// over the two hours it may wait, a dozen or so asks, not one a poll.
		for (let step = 0; step < 6; step++) await later(PULL_RETRY_MAX_MS);
		const grown = asked.length;
		for (let poll = 0; poll < PULL_RETRY_MAX_MS / PULL_POLL_MS - 1; poll++) await later(PULL_POLL_MS);
		expect(asked.length, "no ask within the longest wait").toBe(grown);
		await later(PULL_POLL_MS);
		expect(asked.length).toBe(grown + 1);
		expect(PULL_LIMIT_MS / PULL_RETRY_MAX_MS).toBeLessThan(15);
		expect((await stub().pulls())[0].attempts).toBe(0);
	});

	it("says why it waits, where `newgit status` reads it", async () => {
		await instanceWith("feature");
		setGitHub(() => new Response("down", { status: 503 }));
		await deliver();
		const [waiting] = (await state()).pulls;
		expect(waiting).toMatchObject({ step: "asking GitHub", attempts: 0 });
		expect(waiting.lookupFailed).toContain("GitHub answered 503");
	});

	it("gives up after two hours saying GitHub could not say, and what to do, not to merge an instance", async () => {
		await instanceWith("feature");
		setGitHub(() => new Response("down", { status: 503 }));
		await deliver();
		let steps = 0;
		while ((await stub().pulls()).length > 0) {
			await later(PULL_RETRY_MAX_MS);
			expect(++steps, "it ends").toBeLessThan(20);
		}
		expect(plans, "nothing was landed or refused by the container").toEqual([]);
		const ended = (await state()).ended.at(-1);
		expect(ended).toMatchObject({ target: "github", commit: THEIRS, outcome: "gave up" });
		expect(ended.why).toContain("GitHub still could not say which pull request it came from");
		expect(ended.why).toContain("Recent Deliveries");
		expect(ended.why).toContain("`newgit pull --remote github`");
		expect(ended.why).not.toContain("newgit merge");
		expect(await index()).toEqual(main);
	});

	it("gives up at once, blaming nobody, when there are too many commits to ask about", async () => {
		await instanceWith("feature");
		const commits = Array.from({ length: PULLS_ASKED + 1 }, (_, n) => ({ id: n.toString(16).padStart(40, "0") }));
		await deliver({ payload: push({ commits }) });
		expect(asked).toEqual([]);
		expect(plans).toEqual([]);
		expect(await stub().pulls()).toEqual([]);
		const ended = (await state()).ended.at(-1);
		expect(ended).toMatchObject({ outcome: "gave up" });
		expect(ended.why).toContain(`at most ${PULLS_ASKED}`);
		expect(ended.why).toContain("`newgit pull --remote github`");
		expect(ended.why).not.toContain("feature");
	});

	it("runs the look once, holds no lock, and spends no attempt while it waits", async () => {
		await instanceWith("feature");
		setGitHub(() => new Response("down", { status: 503 }));
		await deliver();
		await later(PULL_RETRY_MAX_MS);
		await later(PULL_RETRY_MAX_MS);
		expect(peeks).toHaveLength(1);
		expect((await call(jack, "POST", path("/branches/main/merge"), { base: main, list: merged })).status).toBe(200);
	});
});
