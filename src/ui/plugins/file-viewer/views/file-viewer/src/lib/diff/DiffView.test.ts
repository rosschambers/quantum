import { describe, test, expect, vi, afterEach, beforeAll } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';

const { callMock } = vi.hoisted(() => ({ callMock: vi.fn() }));
vi.mock('@quantum/client', async (importOriginal) => {
	const actual = await importOriginal<Record<string, unknown>>();
	return { ...actual, createClient: () => ({ call: callMock, subscribe: vi.fn(), close: vi.fn() }) };
});

import DiffView from './DiffView.svelte';
import type { ChangeSet, ChangedFile, FileSide } from '@quantum/client';
import { clearLineDiffCache } from './lineDiffCache';
import { clearRowsCache } from './rowsCache';
import * as rowsModule from './rows';

beforeAll(() => {
	if (typeof (globalThis as any).ResizeObserver === 'undefined') {
		(globalThis as any).ResizeObserver = class {
			observe(): void {}
			unobserve(): void {}
			disconnect(): void {}
		};
	}
	if (typeof Element.prototype.scrollIntoView !== 'function') {
		Element.prototype.scrollIntoView = function (): void {};
	}
});

afterEach(() => {
	cleanup();
	callMock.mockReset();
	vi.restoreAllMocks();
	// `cachedLineDiff`'s memo is module-scoped (by design — it is meant to
	// outlive a single component instance), so a later test reusing the
	// same placeholder blob ids for genuinely different content would
	// otherwise silently read a stale result cached by an earlier test.
	clearLineDiffCache();
	clearRowsCache();
});

function side(content: string, blob: string, overrides: Partial<FileSide> = {}): FileSide {
	return { content, blob, mode: '100644', binary: false, too_large: false, ...overrides };
}

function changeSetFixture(overrides: Partial<ChangeSet> = {}): ChangeSet {
	const file: ChangedFile = {
		path: 'src/lib.rs',
		language: 'rust',
		base: side('old\n', 'base-blob'),
		index: side('old\n', 'base-blob'),
		target: side('new\n', 'target-blob'),
		untracked: false,
	};
	return {
		repository_root: '/repository',
		base_label: 'HEAD',
		target_label: 'working tree',
		stageable: true,
		files: [file],
		...overrides,
	};
}

function mockChangesAndFingerprint(changeSet: ChangeSet, fingerprint = 'fingerprint-1'): void {
	callMock.mockImplementation((method: string) => {
		if (method === 'file-viewer.changes') {
			return Promise.resolve(changeSet);
		}
		if (method === 'file-viewer.fingerprint') {
			return Promise.resolve({ fingerprint });
		}
		return Promise.resolve(undefined);
	});
}

describe('DiffView', () => {
	test('renders entries from a fixture ChangeSet, with a Changes sidebar and a DiffFile per entry', async () => {
		mockChangesAndFingerprint(changeSetFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });

		await vi.waitFor(() => {
			expect(container.querySelector('.changes-sidebar')).not.toBeNull();
		});
		expect(container.querySelectorAll('.diff-file')).toHaveLength(1);
		expect(container.textContent).toContain('src/lib.rs');
	});

	test('stage calls IPC with the displayed blob and moves the file to Staged without a refetch', async () => {
		const changeSet = changeSetFixture({
			files: [
				{
					path: 'src/lib.rs',
					language: 'rust',
					base: side('old\n', 'base-blob'),
					index: side('old\n', 'base-blob'),
					target: side('new\n', 'target-blob'),
					untracked: false,
				},
			],
		});
		mockChangesAndFingerprint(changeSet);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });

		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
		expect(container.textContent).toContain('Unstaged');

		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.stage') return Promise.resolve({});
			if (method === 'file-viewer.changes') return Promise.resolve(changeSet);
			if (method === 'file-viewer.fingerprint') return Promise.resolve({ fingerprint: 'fingerprint-1' });
			return Promise.resolve(undefined);
		});

		const stageButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.trim() === 'Stage');
		expect(stageButton).toBeDefined();
		await fireEvent.click(stageButton!);

		await vi.waitFor(() => {
			expect(callMock).toHaveBeenCalledWith('file-viewer.stage', {
				repository_root: '/repository',
				path: 'src/lib.rs',
				blob: 'target-blob',
				mode: '100644',
			});
		});
		// Refetch never happens: file-viewer.changes was only called once (initial load).
		expect(callMock.mock.calls.filter(([method]) => method === 'file-viewer.changes')).toHaveLength(1);
		await vi.waitFor(() => {
			expect(container.textContent).toContain('\u2713 Staged');
		});
		expect(container.textContent).not.toContain('Unstaged 1');
	});

	test('a stage failure shows the error banner and leaves the entry Unstaged', async () => {
		mockChangesAndFingerprint(changeSetFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.stage') return Promise.reject({ code: -32022, message: 'git failed: fatal error' });
			return Promise.resolve(undefined);
		});

		const stageButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.trim() === 'Stage');
		await fireEvent.click(stageButton!);

		await vi.waitFor(() => {
			expect(container.querySelector('.banner.error')?.textContent).toContain('git failed: fatal error');
		});
		expect(Array.from(container.querySelectorAll('button')).some((button) => button.textContent?.trim() === 'Stage')).toBe(true);
	});

	test('a fingerprint change shows the refresh banner, and R refetches', async () => {
		const changeSet = changeSetFixture();
		let intervalCallback: (() => void) | undefined;
		vi.stubGlobal(
			'setInterval',
			((callback: () => void, ms: number) => {
				expect(ms).toBe(2000);
				intervalCallback = callback;
				return 1 as unknown as ReturnType<typeof setInterval>;
			}) as typeof setInterval,
		);
		vi.stubGlobal('clearInterval', (() => {}) as typeof clearInterval);

		try {
			let fingerprint = 'fingerprint-1';
			callMock.mockImplementation((method: string) => {
				if (method === 'file-viewer.changes') return Promise.resolve(changeSet);
				if (method === 'file-viewer.fingerprint') return Promise.resolve({ fingerprint });
				return Promise.resolve(undefined);
			});
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
			expect(intervalCallback).toBeDefined();

			fingerprint = 'fingerprint-2';
			intervalCallback?.();
			await vi.waitFor(() => {
				expect(container.querySelector('.banner')?.textContent).toContain('Files changed on disk since this diff loaded.');
			});

			await fireEvent.keyDown(window, { key: 'R' });
			await vi.waitFor(() => {
				expect(callMock.mock.calls.filter(([method]) => method === 'file-viewer.changes')).toHaveLength(2);
			});
			expect(container.querySelector('.banner')).toBeNull();
		} finally {
			vi.unstubAllGlobals();
		}
	});

	test('pair mode renders without a sidebar or staging controls', async () => {
		callMock.mockImplementation((method: string, params: any) => {
			if (method === 'file-viewer.read') {
				return Promise.resolve({
					content: params.path === '/a.ts' ? 'left\ncontent\n' : 'right\ncontent\n',
					file_type: 'code',
					filename: params.path,
					directory: '/',
					size: 10,
					language: 'typescript',
					mime_type: null,
					uri: null,
				});
			}
			return Promise.resolve(undefined);
		});
		const { container } = render(DiffView, { props: { source: { kind: 'pair', left: '/a.ts', right: '/b.ts' } } } );
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
		expect(container.querySelector('.changes-sidebar')).toBeNull();
		expect(Array.from(container.querySelectorAll('button')).some((button) => button.textContent?.includes('Stage'))).toBe(false);
	});

	test('"n" scrolls the pane to the next change block', async () => {
		mockChangesAndFingerprint(changeSetFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

		const pane = container.querySelector('.pane') as HTMLElement;
		const rows = Array.from(pane.querySelectorAll('.row.added, .row.removed'));
		expect(rows.length).toBeGreaterThan(0);
		rows.forEach((row, index) => {
			Object.defineProperty(row, 'offsetTop', { value: 100 + index * 50, configurable: true });
		});
		pane.scrollTop = 0;

		await fireEvent.keyDown(window, { key: 'n' });
		expect(pane.scrollTop).toBe(100 - 40);
	});

	test('search counts include lines hidden inside a collapsed region, and navigating to a match expands it', async () => {
		const sameLines = Array.from({ length: 10 }, (_, index) => (index === 5 ? 'needle here' : `same ${index}`));
		const oldContent = ['old start', ...sameLines, 'old end'].join('\n');
		const newContent = ['new start', ...sameLines, 'new end'].join('\n');
		const changeSet = changeSetFixture({
			files: [
				{
					path: 'src/lib.rs',
					language: 'rust',
					base: side(oldContent, 'base-blob'),
					index: side(oldContent, 'base-blob'),
					target: side(newContent, 'target-blob'),
					untracked: false,
				},
			],
		});
		mockChangesAndFingerprint(changeSet);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

		// The needle line sits inside a collapsed context run before any search runs.
		expect(container.querySelector('.collapsed')).not.toBeNull();
		expect(container.textContent).not.toContain('needle here');

		await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
		const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;
		await fireEvent.input(input, { target: { value: 'needle' } });

		await vi.waitFor(() => {
			expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 1');
		});
		// Navigating to the (only, auto-selected) match expanded the region that hid it.
		await vi.waitFor(() => {
			expect(container.textContent).toContain('needle here');
		});
		expect(container.querySelector('.collapsed')).toBeNull();
	});

	test('unstaging a renamed file passes old_path and refetches instead of applying a local mirror', async () => {
		// A staged, pure rename: base is the content under the OLD path,
		// index/target are the SAME renamed content (no further edits) — only
		// a Staged entry exists, exactly the reviewModel.test.ts "renamed and
		// modified" fixture with target pinned equal to index.
		const changeSet = changeSetFixture({
			files: [
				{
					path: 'renamed.ts',
					old_path: 'original.ts',
					language: 'typescript',
					base: side('old\n', 'base-blob'),
					index: side('renamed\n', 'renamed-blob'),
					target: side('renamed\n', 'renamed-blob'),
					untracked: false,
				},
			],
		});
		mockChangesAndFingerprint(changeSet);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

		let changesCallCount = 0;
		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.unstage') return Promise.resolve({});
			if (method === 'file-viewer.changes') {
				changesCallCount++;
				return Promise.resolve(changeSet);
			}
			if (method === 'file-viewer.fingerprint') return Promise.resolve({ fingerprint: 'fingerprint-1' });
			return Promise.resolve(undefined);
		});

		const unstageButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes('Staged'));
		expect(unstageButton).toBeDefined();
		await fireEvent.click(unstageButton!);

		await vi.waitFor(() => {
			expect(callMock).toHaveBeenCalledWith('file-viewer.unstage', {
				repository_root: '/repository',
				path: 'renamed.ts',
				old_path: 'original.ts',
			});
		});
		await vi.waitFor(() => expect(changesCallCount).toBe(1));
	});

	test('a load error (not a repository, git unavailable, ...) shows a full-window error message', async () => {
		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.changes') {
				return Promise.reject({ code: -32020, message: 'not a git repository: /not-a-repo' });
			}
			return Promise.resolve(undefined);
		});
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/not-a-repo' } } } });
		await vi.waitFor(() => {
			expect(container.querySelector('.error-message')?.textContent).toContain('not a git repository: /not-a-repo');
		});
		expect(container.querySelector('.diff-header')).toBeNull();
	});

	test('an empty change set shows "No changes" instead of a blank body', async () => {
		mockChangesAndFingerprint(changeSetFixture({ files: [] }));
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => {
			expect(container.textContent).toContain('No changes');
		});
		expect(container.querySelector('.changes-sidebar')).toBeNull();
	});

	test('Escape closes the search bar before closing the window', async () => {
		mockChangesAndFingerprint(changeSetFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

		await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
		await fireEvent.keyDown(window, { key: 'Escape' });
		expect(container.querySelector('.search-bar')).toBeNull();
		expect(callMock).not.toHaveBeenCalledWith('view.hide', expect.anything());

		await fireEvent.keyDown(window, { key: 'Escape' });
		expect(callMock).toHaveBeenCalledWith('view.hide', expect.anything());
	});

	test('] and [ move the active file and scroll the pane to it', async () => {
		const changeSet = changeSetFixture({
			files: [
				{ path: 'a.ts', language: 'typescript', base: side('old a\n', 'a-base'), index: side('old a\n', 'a-base'), target: side('new a\n', 'a-target'), untracked: false },
				{ path: 'b.ts', language: 'typescript', base: side('old b\n', 'b-base'), index: side('old b\n', 'b-base'), target: side('new b\n', 'b-target'), untracked: false },
			],
		});
		mockChangesAndFingerprint(changeSet);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(2));

		const sections = Array.from(container.querySelectorAll('.file-section'));
		Object.defineProperty(sections[1], 'offsetTop', { value: 500, configurable: true });
		const pane = container.querySelector('.pane') as HTMLElement;

		await fireEvent.keyDown(window, { key: ']' });
		expect(pane.scrollTop).toBe(500);
		expect(container.querySelectorAll('.entry')[1].classList.contains('active')).toBe(true);

		Object.defineProperty(sections[0], 'offsetTop', { value: 0, configurable: true });
		await fireEvent.keyDown(window, { key: '[' });
		expect(pane.scrollTop).toBe(0);
		expect(container.querySelectorAll('.entry')[0].classList.contains('active')).toBe(true);
	});

	test('typing two different search queries never recomputes the always-expanded row list for an entry already diffed', async () => {
		// `ensureMatchVisible` legitimately calls `buildRows` with the
		// entry's REAL (possibly empty) expanded-keys set whenever
		// navigation lands on the new side, to decide whether a fold needs
		// expanding — that call is per-navigation, not per-keystroke-per-
		// entry, and is unrelated to this fix. Distinguish it from the
		// always-expanded calls this fix targets (`buildSearchRows` /
		// the change-marks pass) by probing whether the `expandedKeys`
		// argument reports `true` for an arbitrary key: only the
		// always-expanded stand-in does.
		function isAlwaysExpandedCall(call: unknown[]): boolean {
			const expandedKeys = call[4] as ReadonlySet<string>;
			return expandedKeys.has('__never-a-real-key__');
		}

		const changeSet = changeSetFixture({
			files: [
				{ path: 'a.ts', language: 'typescript', base: side('old a\n', 'a-base'), index: side('old a\n', 'a-base'), target: side('new a\n', 'a-target'), untracked: false },
				{ path: 'b.ts', language: 'typescript', base: side('old b\n', 'b-base'), index: side('old b\n', 'b-base'), target: side('new b\n', 'b-target'), untracked: false },
			],
		});
		mockChangesAndFingerprint(changeSet);
		const buildRowsSpy = vi.spyOn(rowsModule, 'buildRows');
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(2));

		// Mounting already primed the memo (the change-marks pass walks
		// every entry's always-expanded row list once).
		const alwaysExpandedCallsAfterMount = buildRowsSpy.mock.calls.filter(isAlwaysExpandedCall).length;
		expect(alwaysExpandedCallsAfterMount).toBeGreaterThan(0);

		await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
		const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;

		await fireEvent.input(input, { target: { value: 'old' } });
		await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).not.toBeNull());

		// A second, DIFFERENT query against the SAME entries: the
		// always-expanded row list does not depend on the query text, only
		// on each entry's own blobs — so neither keystroke should have
		// grown the always-expanded call count past what mounting alone
		// already produced.
		await fireEvent.input(input, { target: { value: 'new' } });
		await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).not.toBeNull());

		const alwaysExpandedCallsAfterBothQueries = buildRowsSpy.mock.calls.filter(isAlwaysExpandedCall).length;
		expect(alwaysExpandedCallsAfterBothQueries).toBe(alwaysExpandedCallsAfterMount);
	});
});
