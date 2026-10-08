// Turns a `ChangeSet` (the result of `file-viewer.changes`) into the
// entries a diff view's sidebar and file list show, plus the two local
// mutations that keep the view in sync with git after staging/unstaging
// without a refetch. Ported from the qv diff playground's `reviewEntries` /
// `stageFile` / `unstageFile` (docs/playgrounds/qv-diff-playground.html),
// sections mode only — the playground's "one list, state badges" grouping
// was not carried into the settled design.
import type { ChangeSet, ChangedFile, FileSide } from '@quantum/client';

export type ReviewStatus = 'M' | 'A' | 'D' | 'R' | 'U';
export type ReviewSection = 'unstaged' | 'staged' | 'none';

export interface ReviewEntry {
	/** Stable key for this entry (unique per file per section): `${fileId}:${section}`. */
	id: string;
	/** Identity of the underlying file across sections and across local stage/unstage — its current path. */
	fileId: string;
	path: string;
	oldPath?: string;
	language?: string;
	status: ReviewStatus;
	section: ReviewSection;
	/** True only on an Unstaged entry whose file also has a Staged entry (edited again after staging). */
	partiallyStaged: boolean;
	oldSide?: FileSide;
	newSide?: FileSide;
}

/** Two sides are the same change-wise when they compare equal by blob id, never by content; presence must match too. */
function sameSide(a: FileSide | undefined, b: FileSide | undefined): boolean {
	if (a === undefined || b === undefined) {
		return a === undefined && b === undefined;
	}
	return a.blob === b.blob;
}

function cloneSide(side: FileSide | undefined): FileSide | undefined {
	return side ? { ...side } : undefined;
}

function statusFor(file: ChangedFile, oldSide: FileSide | undefined, newSide: FileSide | undefined): ReviewStatus {
	if (file.untracked) {
		return 'U';
	}
	let status: ReviewStatus = 'M';
	if (oldSide === undefined) {
		status = 'A';
	} else if (newSide === undefined) {
		status = 'D';
	}
	// Renames carry `old_path`, but an added or deleted side wins: a rename
	// is only reported as such when the content diff is itself a
	// modification (the backend only sets `old_path` in that case, but this
	// stays defensive against a future change in that contract).
	if (file.old_path && status === 'M') {
		return 'R';
	}
	return status;
}

function makeEntry(
	file: ChangedFile,
	section: ReviewSection,
	oldSide: FileSide | undefined,
	newSide: FileSide | undefined,
	partiallyStaged: boolean,
): ReviewEntry {
	return {
		id: `${file.path}:${section}`,
		fileId: file.path,
		path: file.path,
		oldPath: file.old_path,
		language: file.language,
		status: statusFor(file, oldSide, newSide),
		section,
		partiallyStaged,
		oldSide,
		newSide,
	};
}

function sortEntries(a: ReviewEntry, b: ReviewEntry): number {
	if (a.section !== b.section) {
		// Unstaged sorts before Staged; 'none' (non-stageable sets) never
		// mixes with the other two.
		return a.section === 'unstaged' ? -1 : 1;
	}
	return a.path.localeCompare(b.path);
}

/**
 * Turns a `ChangeSet` into the entries the sidebar and file list show.
 * Stageable sets split each changed file into up to two entries (Unstaged:
 * index → target, or empty → target when untracked; Staged: base → index),
 * each only when its two sides differ by blob id. The Unstaged old side is
 * ALWAYS `index` (never a fallback to `base`): a tracked file with no index
 * side is a staged deletion (`git rm --cached`, index absent, or plain
 * `git rm`, index and target both absent) and the Unstaged comparison must
 * see that absence directly — comparing against `base` instead papers over
 * it and fabricates a phantom Unstaged entry whenever the working tree
 * still matches `base` (plain `git rm`), or reports the wrong status
 * (`M` instead of `A`) when the working tree still has the file (`git rm
 * --cached`). Non-stageable sets show exactly one entry per file (base →
 * target) — git already only lists files that differ, so every file gets
 * an entry there.
 */
export function reviewEntries(changeSet: ChangeSet): ReviewEntry[] {
	if (!changeSet.stageable) {
		return changeSet.files.map((file) => makeEntry(file, 'none', file.base, file.target, false)).sort(sortEntries);
	}

	const entries: ReviewEntry[] = [];
	for (const file of changeSet.files) {
		const hasStaged = !sameSide(file.base, file.index);
		const unstagedOldSide = file.untracked ? undefined : file.index;
		const hasUnstaged = file.untracked || !sameSide(file.index, file.target);

		if (hasUnstaged) {
			entries.push(makeEntry(file, 'unstaged', unstagedOldSide, file.target, hasStaged));
		}
		if (hasStaged) {
			entries.push(makeEntry(file, 'staged', file.base, file.index, false));
		}
	}
	return entries.sort(sortEntries);
}

/** A file counts as done ("staged") when it has no Unstaged entry left. */
export function stagingProgress(entries: readonly ReviewEntry[]): { staged: number; total: number } {
	const files = new Set(entries.map((entry) => entry.fileId));
	const filesWithUnstagedWork = new Set(
		entries.filter((entry) => entry.section === 'unstaged').map((entry) => entry.fileId),
	);
	let staged = 0;
	for (const fileId of files) {
		if (!filesWithUnstagedWork.has(fileId)) {
			staged++;
		}
	}
	return { staged, total: files.size };
}

function updateFile(changeSet: ChangeSet, path: string, updater: (file: ChangedFile) => ChangedFile): ChangeSet {
	return {
		...changeSet,
		files: changeSet.files.map((file) => (file.path === path ? updater(file) : file)),
	};
}

/**
 * Mirrors what `git update-index --add --cacheinfo` now holds after
 * staging the displayed `target` blob (decision D1): the index becomes an
 * exact copy of the target side, and the file is no longer untracked. No
 * refetch, so newer unseen edits on disk never leak in.
 */
export function applyLocalStage(changeSet: ChangeSet, path: string): ChangeSet {
	return updateFile(changeSet, path, (file) => ({
		...file,
		index: cloneSide(file.target),
		untracked: false,
	}));
}

/**
 * Mirrors what `git restore --staged` (or, for a file absent from HEAD,
 * `git rm --cached`) now holds after unstaging: the index reverts to the
 * base side, or is removed and the file returns to untracked when there is
 * no base.
 *
 * Returns `null` when the file carries `old_path` (a staged rename),
 * signalling that the caller must refetch instead of trusting this local
 * mirror. Unstaging a rename restores the OLD path too — the backend
 * passes `old_path` through to the unstage call to do exactly that — so
 * the result is a change spanning two paths (the old path reappearing,
 * the new path's entry going untracked). A single `ChangedFile` is keyed
 * on one path and cannot represent that; mutating it locally would either
 * drop the old path's reappearance or leave the new path in a state git
 * never produces.
 */
export function applyLocalUnstage(changeSet: ChangeSet, path: string): ChangeSet | null {
	const file = changeSet.files.find((candidate) => candidate.path === path);
	if (file?.old_path) {
		return null;
	}
	return updateFile(changeSet, path, (current) => ({
		...current,
		index: current.base ? cloneSide(current.base) : undefined,
		untracked: current.base ? current.untracked : true,
	}));
}
