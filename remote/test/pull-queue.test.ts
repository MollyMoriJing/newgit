import { runDurableObjectAlarm, runInDurableObject } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CHECKING } from "../src/endpoint";
import { ProjectDO } from "../src/project";
import { SWEEP_EVERY_MS } from "../src/sweep";
import { PULL_LIMIT_MS, PULL_POLL_MS, PULL_RETRY_MAX_MS, retryDelay } from "../src/webhook";
import { advance, call, deliver, jack, main, path, setAnswers, setLooks, sha, stub } from "./webhook-fixture";

// A webhook event answered 202 that never landed, without a word (#353):
// the queue is moved on only by `ProjectDO`'s alarm, which set its next
// alarm last, so anything that threw first (a deploy resetting the object
// under a step, Artifacts down during the sweep) left a queued pull with
// nothing to move it until another event set an alarm. A redelivery landed
// it because queueing it again set one. New tests live in their own file per
// AGENTS.md's "Working in parallel".

const state = async () => (await call(jack, "GET", path("/state"))).body;
const alarmAt = () => runInDurableObject(stub(), (_instance, state) => state.storage.getAlarm());
const reset = () => new Error("Durable Object reset because its code was updated.");

let quiet: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
	quiet = vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
	quiet.mockRestore();
	vi.useRealTimers();
});

describe("the queue's alarm", () => {
	it("is armed before the step, and keeps going when a step throws outside the pull's own", async () => {
		await deliver({ hold: true });
		// The alarm's real deps have no container, so the step fails; counting
		// that failure is where a reset cuts it off here.
		let armed: number | null = null;
		const cut = vi.spyOn(ProjectDO.prototype, "pullFailed").mockImplementationOnce(async function (this: ProjectDO) {
			armed = await (this as unknown as { ctx: DurableObjectState }).ctx.storage.getAlarm();
			throw reset();
		} as never);
		expect(await runDurableObjectAlarm(stub())).toBe(true);
		cut.mockRestore();
		expect(armed, "set before the step's first await: a step cut off still has a next alarm").not.toBeNull();
		expect(await alarmAt(), "armed again: the queue is not left to sit").not.toBeNull();
		const [waiting] = (await state()).pulls;
		expect(waiting.lastError, "and it says why it did not move").toContain("reset");

		vi.useFakeTimers({ toFake: ["Date"] });
		vi.setSystemTime(Date.now() + PULL_POLL_MS);
		expect(await runDurableObjectAlarm(stub())).toBe(true);
		expect((await stub().pulls())[0].attempts, "the next alarm carries on").toBe(1);
	});

	it("moves the queue on when the sweep fails", async () => {
		vi.useFakeTimers({ toFake: ["Date"] });
		await stub().sweepSoon();
		const down = vi.spyOn(ProjectDO.prototype, "pendingBranches").mockImplementation(() => {
			throw new Error("Artifacts is down");
		});
		vi.setSystemTime(Date.now() + SWEEP_EVERY_MS);
		await deliver({ hold: true });
		expect(await runDurableObjectAlarm(stub())).toBe(true);
		down.mockRestore();
		expect((await stub().pulls())[0].attempts, "the pull's step ran").toBe(1);
		expect(quiet.mock.calls.flat().join("\n")).toContain("Artifacts is down");
		const next = await alarmAt();
		expect(next).not.toBeNull();
		expect(next! - Date.now(), "polled again soon, not left for the sweep").toBeLessThanOrEqual(PULL_POLL_MS);
	});
});

describe("a stalled queue", () => {
	it("is said, and asking for the state starts it again", async () => {
		await deliver({ hold: true });
		await runInDurableObject(stub(), (_instance, state) => state.storage.deleteAlarm());
		const [waiting] = (await state()).pulls;
		expect(waiting).toMatchObject({ step: "looking", stalled: true });
		expect(await alarmAt(), "re-armed").not.toBeNull();
		expect((await state()).pulls[0].stalled, "said once: it is moving now").toBeUndefined();
	});
});

describe("no pull ends without a word", () => {
	it("says a pull that waited two hours on the container gave up, and what to do", async () => {
		setLooks(Array.from({ length: 1000 }, () => CHECKING));
		await deliver();
		vi.useFakeTimers({ toFake: ["Date"] });
		vi.setSystemTime(Date.now() + PULL_LIMIT_MS + 1000);
		expect(await advance()).toBe(false);
		expect(await stub().pulls()).toEqual([]);
		const ended = (await state()).ended.at(-1);
		expect(ended).toMatchObject({ outcome: "gave up" });
		expect(ended.why).toContain("without finishing");
		expect(ended.why).toContain("Recent Deliveries");
	});

	it("says an event that found nothing to bring in, which ends an earlier refusal", async () => {
		setAnswers([{ outcome: "refused", why: "commit 123 changes `apps/t3code`" }]);
		await deliver();
		setLooks([{ outcome: "nothing" }]);
		await deliver({ payload: { ref: "refs/heads/main", after: sha("c"), repository: { full_name: "Spheroman/newgit" }, pusher: { name: "jack" }, commits: [{ id: sha("c") }] } });
		const ended = (await state()).ended;
		expect(ended.map((one: { outcome: string }) => one.outcome)).toEqual(["refused", "nothing"]);
	});
});

describe("a failure", () => {
	it("is tried again later each time, and the wait is said", async () => {
		setAnswers(Array.from({ length: 4 }, () => new Error("Network connection lost.")));
		vi.useFakeTimers({ toFake: ["Date"] });
		await deliver();
		expect((await stub().pulls())[0].attempts).toBe(1);
		const [waiting] = (await state()).pulls;
		expect(waiting.lastError).toContain("Network connection lost");
		expect(Date.parse(waiting.retryAt) - Date.now()).toBe(retryDelay(1));

		// Within the wait nothing runs; after it, the next attempt.
		vi.setSystemTime(Date.now() + retryDelay(1) - 1);
		await advance();
		expect((await stub().pulls())[0].attempts).toBe(1);
		vi.setSystemTime(Date.now() + 1);
		await advance();
		expect((await stub().pulls())[0].attempts).toBe(2);
		expect(retryDelay(2)).toBe(2 * retryDelay(1));
		expect(retryDelay(100)).toBe(PULL_RETRY_MAX_MS);
	});

	it("finishes once the network is back (#389)", async () => {
		setAnswers([
			new Error("Network connection lost."),
			new Error("Network connection lost."),
			new Error("Network connection lost."),
			{ outcome: "landed", list: { ...main, root: sha("c") }, commits: 1 },
		]);
		vi.useFakeTimers({ toFake: ["Date"] });
		await deliver();
		for (let step = 0; step < 3; step++) {
			vi.setSystemTime(Date.now() + PULL_RETRY_MAX_MS);
			await advance();
		}
		expect(await stub().pulls()).toEqual([]);
		expect((await state()).ended.at(-1)).toMatchObject({ outcome: "landed" });
	});
});
