import { describe, expect, it } from "vitest";

import { ArtifactsRepos } from "../src/artifacts";
import { nativeAncestry } from "../src/walk";

// `ArtifactsRepos` over a stand-in for the Artifacts binding that resolves
// refs as the live service does: `log` takes a branch by its short name, a
// tag, or a commit id, and answers an empty log for anything else, a full
// `refs/heads/<branch>` and `HEAD` included. `FakeRepos` sits above this
// adapter, so a ref spelled the way Artifacts does not resolve passed every
// test there while every branch read as having no head live: no tag could
// be made, since none was ever on `main`.
// New tests live in their own file per AGENTS.md's "Working in parallel".

const sha = (c: string) => c.repeat(40);

interface StoredCommit {
	hash: string;
	treeHash: string;
	parents: string[];
	committedAt: number;
}

/** One repo's commits and branches, as Artifacts' binding hands them out. */
function binding(repos: Record<string, { commits: StoredCommit[]; branches: Record<string, string> }>) {
	const asked: string[] = [];
	const artifacts = {
		async get(name: string) {
			const repo = repos[name];
			if (repo === undefined) throw Object.assign(new Error(`${name} not found`), { code: "NOT_FOUND" });
			const byHash = new Map(repo.commits.map((commit) => [commit.hash, commit]));
			return {
				// The real binding is disposable (`using`); the test lib types lack `Symbol.dispose`.
				[(Symbol as unknown as { dispose: symbol }).dispose]() {},
				async readCommit(hash: string) {
					return byHash.get(hash) ?? null;
				},
				async log({ ref = "HEAD", limit = 50 }: { ref?: string; limit?: number } = {}) {
					asked.push(ref);
					let at = repo.branches[ref] ?? (byHash.has(ref) ? ref : undefined);
					const out: StoredCommit[] = [];
					while (at !== undefined && out.length < limit) {
						const commit = byHash.get(at)!;
						out.push(commit);
						at = commit.parents[0];
					}
					return out;
				},
			};
		},
	};
	return { repos: new ArtifactsRepos(artifacts as unknown as Artifacts, "https://git.example"), asked };
}

/** Root's history: a merge on `main` whose second parent came in from a mirror. */
function rootHistory() {
	const at = (n: number) => 1_800_000_000 + n * 60;
	const commits: StoredCommit[] = [
		{ hash: sha("1"), treeHash: sha("9"), parents: [], committedAt: at(0) },
		// The release, landed from the mirror by the webhook.
		{ hash: sha("2"), treeHash: sha("9"), parents: [sha("1")], committedAt: at(1) },
		{ hash: sha("3"), treeHash: sha("9"), parents: [sha("1")], committedAt: at(2) },
		{ hash: sha("4"), treeHash: sha("9"), parents: [sha("3"), sha("2")], committedAt: at(3) },
		{ hash: sha("5"), treeHash: sha("9"), parents: [sha("4")], committedAt: at(4) },
		// Work in flight on another branch.
		{ hash: sha("6"), treeHash: sha("9"), parents: [sha("5")], committedAt: at(5) },
	];
	return { root: { commits, branches: { main: sha("5"), work: sha("6") } } };
}

describe("a branch's head, read from Artifacts", () => {
	it("is asked for by the branch's short name", async () => {
		const { repos, asked } = binding(rootHistory());
		expect(await repos.branchHead("root", "main")).toBe(sha("5"));
		expect(await repos.branchHead("root", "work")).toBe(sha("6"));
		expect(asked).toEqual(["main", "work"]);
	});

	it("is null for a branch the repo does not have", async () => {
		const { repos } = binding(rootHistory());
		expect(await repos.branchHead("root", "gone")).toBeNull();
		expect(await repos.branchHead("nowhere", "main")).toBeNull();
	});
});

describe("whether a commit is on `main`, read from Artifacts", () => {
	const onMain = async (commit: string) => {
		const { repos } = binding(rootHistory());
		const answers = await nativeAncestry(repos, (tracker) => tracker, [{ tracker: "root", old: commit, new: commit }]);
		if (!Array.isArray(answers)) throw new Error("the walk asked Git");
		return answers[0].onMain;
	};

	it("is yes for every commit of its history, the tip and ones far back", async () => {
		expect(await onMain(sha("5"))).toBe(true);
		expect(await onMain(sha("3"))).toBe(true);
		expect(await onMain(sha("1"))).toBe(true);
	});

	it("is yes for one a mirror's pull request brought in, behind a merge", async () => {
		expect(await onMain(sha("2"))).toBe(true);
	});

	it("is no for work `main` does not have", async () => {
		expect(await onMain(sha("6"))).toBe(false);
	});
});
