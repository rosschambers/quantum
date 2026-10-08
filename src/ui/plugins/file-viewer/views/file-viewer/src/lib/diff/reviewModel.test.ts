import { describe, test, expect } from 'vitest';
import type { ChangeSet, ChangedFile, FileSide } from '@quantum/client';
import { reviewEntries, stagingProgress, applyLocalStage, applyLocalUnstage } from './reviewModel';

function side(blob: string): FileSide {
	return { blob, mode: '100644', binary: false, too_large: false };
}

function stageableChangeSet(files: ChangedFile[]): ChangeSet {
	return {
		repository_root: '/repository',
		base_label: 'HEAD',
		target_label: 'working tree',
		stageable: true,
		files,
	};
}

describe('reviewEntries', () => {
	test('a partially staged file appears in both sections, partiallyStaged only on its Unstaged entry', () => {
		const file: ChangedFile = {
			path: 'src/lib.rs',
			language: 'rust',
			base: side('b1'),
			index: side('b2'),
			target: side('b3'),
			untracked: false,
		};
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(2);

		const unstaged = entries.find((entry) => entry.section === 'unstaged')!;
		expect(unstaged.status).toBe('M');
		expect(unstaged.partiallyStaged).toBe(true);
		expect(unstaged.oldSide?.blob).toBe('b2');
		expect(unstaged.newSide?.blob).toBe('b3');

		const staged = entries.find((entry) => entry.section === 'staged')!;
		expect(staged.status).toBe('M');
		expect(staged.partiallyStaged).toBe(false);
		expect(staged.oldSide?.blob).toBe('b1');
		expect(staged.newSide?.blob).toBe('b2');
	});

	test('an untracked file gets status U and only an Unstaged entry', () => {
		const file: ChangedFile = { path: 'new.ts', target: side('t1'), untracked: true };
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(1);
		expect(entries[0]).toMatchObject({ section: 'unstaged', status: 'U', partiallyStaged: false });
		expect(entries[0].oldSide).toBeUndefined();
		expect(entries[0].newSide?.blob).toBe('t1');
	});

	test('a renamed and modified file gets status R and carries oldPath', () => {
		const file: ChangedFile = {
			path: 'renamed.ts',
			old_path: 'original.ts',
			base: side('b1'),
			index: side('b1'),
			target: side('b2'),
			untracked: false,
		};
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(1);
		expect(entries[0].status).toBe('R');
		expect(entries[0].oldPath).toBe('original.ts');
	});

	test('a deleted file gets status D', () => {
		const file: ChangedFile = { path: 'gone.ts', base: side('b1'), index: side('b1'), untracked: false };
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(1);
		expect(entries[0].status).toBe('D');
		expect(entries[0].section).toBe('unstaged');
	});

	test('an added-then-staged-unchanged file produces only a Staged entry, status A', () => {
		const file: ChangedFile = { path: 'added.ts', index: side('i1'), target: side('i1'), untracked: false };
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(1);
		expect(entries[0]).toMatchObject({ section: 'staged', status: 'A' });
	});

	test('Unstaged entries sort before Staged, and within a section by path', () => {
		const partial: ChangedFile = {
			path: 'b.ts',
			base: side('b1'),
			index: side('b2'),
			target: side('b3'),
			untracked: false,
		};
		// Fully staged with no further edits: index equals target, so there
		// is no Unstaged entry for this file, only a Staged one.
		const stagedOnly: ChangedFile = { path: 'a.ts', base: side('c1'), index: side('c2'), target: side('c2'), untracked: false };
		const entries = reviewEntries(stageableChangeSet([partial, stagedOnly]));
		expect(entries.map((entry) => `${entry.section}:${entry.path}`)).toEqual([
			'unstaged:b.ts',
			'staged:a.ts',
			'staged:b.ts',
		]);
	});

	test('a tracked file with no index side (a staged deletion) falls back to base as the Unstaged old side', () => {
		// A tracked file can only have an absent index side via `git rm
		// --cached` (a staged deletion), so this always also produces a
		// Staged `D` entry; the Unstaged entry's old side falls back to base.
		const file: ChangedFile = { path: 'weird.ts', base: side('b1'), target: side('b2'), untracked: false };
		const entries = reviewEntries(stageableChangeSet([file]));
		expect(entries).toHaveLength(2);
		const unstaged = entries.find((entry) => entry.section === 'unstaged')!;
		expect(unstaged).toMatchObject({ status: 'M', partiallyStaged: true });
		expect(unstaged.oldSide?.blob).toBe('b1');
		const staged = entries.find((entry) => entry.section === 'staged')!;
		expect(staged.status).toBe('D');
	});

	test('non-stageable sets produce exactly one entry per file, sorted by path', () => {
		const changeSet: ChangeSet = {
			repository_root: '/repository',
			base_label: 'abc123',
			target_label: 'working tree',
			stageable: false,
			files: [
				{ path: 'z.ts', base: side('b1'), target: side('b2'), untracked: false },
				{ path: 'a.ts', target: side('b3'), untracked: false },
			],
		};
		const entries = reviewEntries(changeSet);
		expect(entries.map((entry) => entry.path)).toEqual(['a.ts', 'z.ts']);
		expect(entries.every((entry) => entry.section === 'none')).toBe(true);
		expect(entries.find((entry) => entry.path === 'a.ts')!.status).toBe('A');
	});
});

describe('stagingProgress', () => {
	test('counts a file as staged only when it has no Unstaged entry', () => {
		const entries = reviewEntries(
			stageableChangeSet([
				{ path: 'partial.ts', base: side('b1'), index: side('b2'), target: side('b3'), untracked: false },
				{ path: 'done.ts', base: side('c1'), index: side('c2'), target: side('c2'), untracked: false },
				{ path: 'todo.ts', base: side('d1'), index: side('d1'), target: side('d2'), untracked: false },
			]),
		);
		expect(stagingProgress(entries)).toEqual({ staged: 1, total: 3 });
	});

	test('an empty entry list has zero of zero', () => {
		expect(stagingProgress([])).toEqual({ staged: 0, total: 0 });
	});
});

describe('applyLocalStage / applyLocalUnstage', () => {
	test('staging an untracked file moves it to Staged without a refetch', () => {
		const changeSet = stageableChangeSet([{ path: 'new.ts', target: side('t1'), untracked: true }]);
		const staged = applyLocalStage(changeSet, 'new.ts');
		const entries = reviewEntries(staged);
		expect(entries).toHaveLength(1);
		expect(entries[0]).toMatchObject({ section: 'staged', status: 'A' });
		expect(staged.files[0].index?.blob).toBe('t1');
		expect(staged.files[0].untracked).toBe(false);
	});

	test('unstaging that same file returns it to untracked with no index side', () => {
		const changeSet = stageableChangeSet([{ path: 'new.ts', target: side('t1'), untracked: true }]);
		const staged = applyLocalStage(changeSet, 'new.ts');
		const unstaged = applyLocalUnstage(staged, 'new.ts');
		expect(unstaged.files[0].index).toBeUndefined();
		expect(unstaged.files[0].untracked).toBe(true);
		const entries = reviewEntries(unstaged);
		expect(entries).toHaveLength(1);
		expect(entries[0]).toMatchObject({ section: 'unstaged', status: 'U' });
	});

	test('staging a modified tracked file sets the index to the displayed target blob', () => {
		const changeSet = stageableChangeSet([
			{ path: 'lib.rs', base: side('b1'), index: side('b1'), target: side('b2'), untracked: false },
		]);
		const staged = applyLocalStage(changeSet, 'lib.rs');
		expect(staged.files[0].index?.blob).toBe('b2');
		const entries = reviewEntries(staged);
		expect(entries).toHaveLength(1);
		expect(entries[0].section).toBe('staged');
	});

	test('unstaging a staged modification reverts the index to base', () => {
		const changeSet = stageableChangeSet([
			{ path: 'lib.rs', base: side('b1'), index: side('b2'), target: side('b2'), untracked: false },
		]);
		const unstaged = applyLocalUnstage(changeSet, 'lib.rs');
		expect(unstaged.files[0].index?.blob).toBe('b1');
		const entries = reviewEntries(unstaged);
		expect(entries).toHaveLength(1);
		expect(entries[0].section).toBe('unstaged');
	});

	test('applying to an unrelated path leaves the file unchanged', () => {
		const changeSet = stageableChangeSet([
			{ path: 'lib.rs', base: side('b1'), index: side('b1'), target: side('b2'), untracked: false },
		]);
		const result = applyLocalStage(changeSet, 'other.rs');
		expect(result.files[0]).toEqual(changeSet.files[0]);
	});
});
