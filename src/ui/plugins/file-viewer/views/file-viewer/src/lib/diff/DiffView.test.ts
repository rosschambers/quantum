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

function rect(partial: Partial<DOMRect>): DOMRect {
	return {
		x: 0,
		y: 0,
		width: 0,
		height: 0,
		top: 0,
		left: 0,
		right: 0,
		bottom: 0,
		toJSON() {
			return this;
		},
		...partial,
	} as DOMRect;
}

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

	test('the header counts every file with anything staged and how many are only partly staged', async () => {
		mockChangesAndFingerprint(
			changeSetFixture({
				files: [
					// Partly staged: staged once, then edited again.
					{ path: 'partial.rs', base: side('a\n', 'p1'), index: side('b\n', 'p2'), target: side('c\n', 'p3'), untracked: false },
					// Fully staged.
					{ path: 'done.rs', base: side('a\n', 'd1'), index: side('b\n', 'd2'), target: side('b\n', 'd2'), untracked: false },
					// Nothing staged.
					{ path: 'todo.rs', base: side('a\n', 't1'), index: side('a\n', 't1'), target: side('b\n', 't2'), untracked: false },
				],
			}),
		);
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });

		await vi.waitFor(() => expect(container.querySelector('.progress')).not.toBeNull());
		expect(container.querySelector('.progress')?.textContent?.replace(/\s+/g, ' ').trim()).toBe('2 staged · 1 partly');
	});

	test('the header omits the partly count when nothing is partly staged', async () => {
		mockChangesAndFingerprint(changeSetFixture());
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });

		await vi.waitFor(() => expect(container.querySelector('.progress')).not.toBeNull());
		expect(container.querySelector('.progress')?.textContent?.replace(/\s+/g, ' ').trim()).toBe('0 staged');
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
		// The overview ruler's remeasure pass schedules itself via the REAL
		// `requestAnimationFrame`, which jsdom's own polyfill implements on
		// top of the global `setInterval` — colliding with this test's own
		// stub of `setInterval` (meant to capture only the poll timer
		// below). Stubbing `requestAnimationFrame` to a no-op keeps the
		// ruler's scheduling out of this test's `setInterval` capture.
		vi.stubGlobal('requestAnimationFrame', (() => 1) as typeof requestAnimationFrame);
		vi.stubGlobal('cancelAnimationFrame', (() => {}) as typeof cancelAnimationFrame);
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

	test('a poll tick whose response lands after the polling effect is torn down writes no state', async () => {
		// Models "teardown before the fingerprint promise resolves": the
		// repository root changes (a new diff is loaded into the SAME
		// component instance — the same effect-cleanup path real view
		// teardown exercises), tearing down the FIRST polling effect
		// instance and its in-flight request before that request's
		// response ever arrives. The component stays mounted so the
		// resulting (lack of) state change is directly observable in the
		// rendered banner.
		const firstChangeSet = changeSetFixture({ repository_root: '/repository-one' });
		const secondChangeSet = changeSetFixture({ repository_root: '/repository-two' });
		let intervalCallback: (() => void) | undefined;
		// See the matching comment in "a fingerprint change shows the
		// refresh banner" above: the ruler's own `requestAnimationFrame`
		// scheduling must not collide with this test's `setInterval` stub.
		vi.stubGlobal('requestAnimationFrame', (() => 1) as typeof requestAnimationFrame);
		vi.stubGlobal('cancelAnimationFrame', (() => {}) as typeof cancelAnimationFrame);
		vi.stubGlobal(
			'setInterval',
			((callback: () => void) => {
				intervalCallback = callback;
				return 1 as unknown as ReturnType<typeof setInterval>;
			}) as typeof setInterval,
		);
		const clearIntervalSpy = vi.fn();
		vi.stubGlobal('clearInterval', clearIntervalSpy as typeof clearInterval);

		try {
			let fingerprintCallCount = 0;
			let resolveStalePoll: ((value: { fingerprint: string }) => void) | undefined;
			callMock.mockImplementation((method: string, params: any) => {
				if (method === 'file-viewer.changes') {
					return Promise.resolve(params.repository === '/repository-two' ? secondChangeSet : firstChangeSet);
				}
				if (method === 'file-viewer.fingerprint') {
					fingerprintCallCount++;
					if (params.repository_root === '/repository-two') {
						return Promise.resolve({ fingerprint: 'second-fingerprint-1' });
					}
					if (fingerprintCallCount === 1) {
						return Promise.resolve({ fingerprint: 'first-fingerprint-1' });
					}
					// The FIRST repository's poll tick: left hanging so the
					// repository switch below can tear down its owning
					// effect before this resolves.
					return new Promise<{ fingerprint: string }>((resolve) => {
						resolveStalePoll = resolve;
					});
				}
				return Promise.resolve(undefined);
			});

			const { container, rerender } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository-one' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
			expect(intervalCallback).toBeDefined();

			// Kick off the first repository's poll tick; it hangs.
			intervalCallback?.();
			await vi.waitFor(() => expect(resolveStalePoll).toBeDefined());

			// Switch to a different repository: the polling effect's
			// dependency (the stable root captured at load) changes, so
			// Svelte tears down THIS effect instance (running its cleanup)
			// before starting a fresh one for the new root.
			await rerender({ source: { kind: 'git', spec: { repository: '/repository-two' } } });
			await vi.waitFor(() => expect(container.textContent).toContain('/repository-two'));
			expect(clearIntervalSpy).toHaveBeenCalled();

			// The FIRST repository's stale poll response finally arrives,
			// reporting a fingerprint that would have differed from its
			// OWN baseline — but its owning effect was already torn down,
			// so this must write nothing observable.
			resolveStalePoll?.({ fingerprint: 'first-fingerprint-2' });
			await vi.waitFor(() => expect(container.querySelector('.banner')).not.toBeNull(), { timeout: 300 }).catch(() => {});
			expect(container.querySelector('.banner')).toBeNull();
		} finally {
			vi.unstubAllGlobals();
		}
	});

	test('a poll response whose baseline changed while it was in flight (a refresh raced it) is ignored', async () => {
		const changeSet = changeSetFixture();
		let intervalCallback: (() => void) | undefined;
		// See the matching comment in "a fingerprint change shows the
		// refresh banner" above.
		vi.stubGlobal('requestAnimationFrame', (() => 1) as typeof requestAnimationFrame);
		vi.stubGlobal('cancelAnimationFrame', (() => {}) as typeof cancelAnimationFrame);
		vi.stubGlobal(
			'setInterval',
			((callback: () => void) => {
				intervalCallback = callback;
				return 1 as unknown as ReturnType<typeof setInterval>;
			}) as typeof setInterval,
		);
		vi.stubGlobal('clearInterval', (() => {}) as typeof clearInterval);

		try {
			let fingerprintCallCount = 0;
			let resolveStalePoll: ((value: { fingerprint: string }) => void) | undefined;
			callMock.mockImplementation((method: string) => {
				if (method === 'file-viewer.changes') return Promise.resolve(changeSet);
				if (method === 'file-viewer.fingerprint') {
					fingerprintCallCount++;
					if (fingerprintCallCount === 1) {
						return Promise.resolve({ fingerprint: 'fingerprint-1' });
					}
					if (fingerprintCallCount === 2) {
						// The poll tick this test drives: left hanging so a
						// refresh can land and move the baseline before it
						// resolves.
						return new Promise<{ fingerprint: string }>((resolve) => {
							resolveStalePoll = resolve;
						});
					}
					// The REFRESH's own fingerprint re-fetch (`refresh()`
					// below), establishing the new baseline immediately.
					return Promise.resolve({ fingerprint: 'fingerprint-3' });
				}
				return Promise.resolve(undefined);
			});

			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
			expect(intervalCallback).toBeDefined();

			// Kick off the poll tick; it hangs on fingerprintCallCount === 2.
			intervalCallback?.();
			await vi.waitFor(() => expect(resolveStalePoll).toBeDefined());

			// A refresh races it, moving the baseline to "fingerprint-3"
			// while the poll's request for the OLD baseline is still in flight.
			await fireEvent.keyDown(window, { key: 'R' });
			await vi.waitFor(() => {
				expect(callMock.mock.calls.filter(([method]) => method === 'file-viewer.fingerprint')).toHaveLength(3);
			});
			// Let the refresh's own promise chain (its `file-viewer.changes`
			// await, then its `file-viewer.fingerprint` await and the
			// `baselineFingerprint` assignment) fully settle before the
			// stale poll resolves, so there is no ambiguity about which
			// write lands last.
			for (let flush = 0; flush < 10; flush++) {
				await Promise.resolve();
			}
			expect(container.querySelector('.banner')).toBeNull();

			// The stale poll's response finally arrives, reporting a
			// fingerprint that differs from the baseline it was ORIGINALLY
			// compared against ("fingerprint-1") — but that baseline is no
			// longer current, so this must not raise the banner.
			resolveStalePoll?.({ fingerprint: 'fingerprint-2' });
			await vi.waitFor(() => expect(container.querySelector('.banner')).not.toBeNull(), { timeout: 300 }).catch(() => {});
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

		const unstageButton = container.querySelector<HTMLButtonElement>('.diff-file .stage-button.staged');
		expect(unstageButton).not.toBeNull();
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

	test('git being absent from quantumd\'s PATH shows a full-window error message naming PATH', async () => {
		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.changes') {
				return Promise.reject({ code: -32021, message: "git is not available on quantumd's PATH: No such file or directory (os error 2)" });
			}
			return Promise.resolve(undefined);
		});
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => {
			expect(container.querySelector('.error-message')?.textContent).toContain('PATH');
		});
		expect(container.querySelector('.diff-header')).toBeNull();
	});

	test('a failed git invocation shows a full-window error message with its stderr', async () => {
		callMock.mockImplementation((method: string) => {
			if (method === 'file-viewer.changes') {
				return Promise.reject({ code: -32022, message: 'git failed: fatal: ambiguous argument \'HEAD\': unknown revision' });
			}
			return Promise.resolve(undefined);
		});
		const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
		await vi.waitFor(() => {
			expect(container.querySelector('.error-message')?.textContent).toContain("fatal: ambiguous argument 'HEAD': unknown revision");
		});
		expect(container.querySelector('.diff-header')).toBeNull();
	});

	test('pair mode shows a full-window error message when one side fails to read', async () => {
		callMock.mockImplementation((method: string, params: any) => {
			if (method === 'file-viewer.read') {
				if (params.path === '/missing.ts') {
					return Promise.reject({ code: -32005, message: 'not found: /missing.ts' });
				}
				return Promise.resolve({
					content: 'content\n',
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
		const { container } = render(DiffView, { props: { source: { kind: 'pair', left: '/missing.ts', right: '/b.ts' } } });
		await vi.waitFor(() => {
			expect(container.querySelector('.error-message')?.textContent).toContain('not found: /missing.ts');
		});
		expect(container.querySelector('.diff-header')).toBeNull();
	});

	test('pair mode of two image files renders the binary placeholder and no staging controls', async () => {
		callMock.mockImplementation((method: string, params: any) => {
			if (method === 'file-viewer.read') {
				return Promise.resolve({
					content: '',
					file_type: 'image',
					filename: params.path,
					directory: '/',
					size: 2048,
					mime_type: 'image/png',
					uri: `file://${params.path}`,
				});
			}
			return Promise.resolve(undefined);
		});
		const { container } = render(DiffView, { props: { source: { kind: 'pair', left: '/a.png', right: '/b.png' } } });
		await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
		expect(container.querySelector('.placeholder')?.textContent).toContain('Binary file changed');
		expect(container.querySelector('.changes-sidebar')).toBeNull();
		expect(Array.from(container.querySelectorAll('button')).some((button) => button.textContent?.includes('Stage'))).toBe(false);
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

describe('DiffView overview ruler measurement', () => {
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

	// The production code schedules its remeasure pass through
	// `coalesceToAnimationFrame` (shared with the resize-watch path), whose
	// `schedule()` assigns `pendingFrame = requestAnimationFrame(wrapped)`
	// — if `requestAnimationFrame` ran `wrapped` SYNCHRONOUSLY (invoking it
	// before returning), that inner call's own `pendingFrame = null` would
	// be clobbered by the outer assignment completing afterward, wedging
	// the coalescer closed forever. A real browser's `requestAnimationFrame`
	// never does that (it always defers to the next frame), so this can
	// never happen in production; deferring by one microtask here keeps
	// the stub a faithful "soon, but not synchronously" rAF, which
	// `vi.waitFor`'s real-timer polling below always observes.
	function stubAnimationFrame(): void {
		vi.stubGlobal('requestAnimationFrame', ((callback: FrameRequestCallback) => {
			queueMicrotask(() => callback(0));
			return 1;
		}) as typeof requestAnimationFrame);
		vi.stubGlobal('cancelAnimationFrame', (() => {}) as typeof cancelAnimationFrame);
	}

	function fileSectionIndexOf(element: Element): number {
		const section = element.closest('.file-section');
		if (!section) return -1;
		return Array.from(document.querySelectorAll('.file-section')).indexOf(section);
	}

	function stubTrackHeight(height: number): void {
		vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (this: HTMLElement) {
			return this.classList.contains('overview-ruler') ? height : 0;
		});
	}

	afterEach(() => {
		vi.unstubAllGlobals();
	});

	test('two files of very different heights produce ruler marks proportional to their MEASURED tops, not an equal share per file', async () => {
		stubAnimationFrame();
		stubTrackHeight(1000);

		// Entry A: one removed+added pair at index 0 (no leading context).
		// Entry B: five leading (unfolded — hiddenCount 2 stays under the
		// collapse threshold) context lines, so its own removed+added pair
		// sits at index 5. Distinct indices let the stub below tell the
		// two files' rows apart without needing a real layout engine.
		const changeSet = changeSetFixture({
			files: [
				{ path: 'a.ts', language: 'typescript', base: side('old a\n', 'a-base'), index: side('old a\n', 'a-base'), target: side('new a\n', 'a-target'), untracked: false },
				{
					path: 'b.ts',
					language: 'typescript',
					base: side(['ctx0', 'ctx1', 'ctx2', 'ctx3', 'ctx4', 'old b'].join('\n'), 'b-base'),
					index: side(['ctx0', 'ctx1', 'ctx2', 'ctx3', 'ctx4', 'old b'].join('\n'), 'b-base'),
					target: side(['ctx0', 'ctx1', 'ctx2', 'ctx3', 'ctx4', 'new b'].join('\n'), 'b-target'),
					untracked: false,
				},
			],
		});
		mockChangesAndFingerprint(changeSet);

		// INVERTED relative to file order on purpose: file A (entryIndex 0,
		// which the old equal-share arithmetic would have forced into the
		// top half, [0, 0.5)) is stubbed near the BOTTOM of the content;
		// file B (entryIndex 1, forced into the bottom half, [0.5, 1)) is
		// stubbed near the TOP. Only a real measurement pass — never
		// `(entryIndex + fraction) / entries.length` — can produce ruler
		// marks that land in the inverted order this test asserts.
		const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
			if (this.classList.contains('pane-content')) return rect({ top: 0, height: 2000 });
			const fileIndex = fileSectionIndexOf(this);
			if (fileIndex === 0 && this.dataset.oldIndex === '0') return rect({ top: 1800, height: 20 });
			if (fileIndex === 0 && this.dataset.newIndex === '0') return rect({ top: 1820, height: 20 });
			if (fileIndex === 1 && this.dataset.oldIndex === '5') return rect({ top: 100, height: 20 });
			if (fileIndex === 1 && this.dataset.newIndex === '5') return rect({ top: 120, height: 20 });
			return rect({ top: 0, height: 0 });
		});

		try {
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(2));

			const mixedMarks = Array.from(container.querySelectorAll('.ruler-mark.kind-mixed')) as HTMLElement[];
			expect(mixedMarks).toHaveLength(2);
			const tops = mixedMarks.map((element) => parseFloat(element.style.top)).sort((a, b) => a - b);
			// B's mark (measured top 100/2000 = 0.05 of the 1000px track = 50px).
			expect(tops[0]).toBeCloseTo(50, 0);
			// A's mark (measured top 1800/2000 = 0.9 of the 1000px track = 900px).
			expect(tops[1]).toBeCloseTo(900, 0);
		} finally {
			rectSpy.mockRestore();
		}
	});

	test('a staged (collapsed) file\'s marks anchor at its header, since nothing else is rendered', async () => {
		stubAnimationFrame();
		stubTrackHeight(1000);

		// Two matching files: the first (unstaged) becomes the auto-selected
		// CURRENT match and is irrelevant here. The second is staged
		// (collapsed by default) — its match is never navigated to, so
		// `ensureMatchVisible`'s "expand on navigation" behavior (a
		// pre-existing, separate feature) never kicks in and the file
		// stays genuinely collapsed for this assertion.
		const changeSet = changeSetFixture({
			files: [
				{ path: 'a.ts', language: 'typescript', base: side('old\n', 'a-base'), index: side('needle one\n', 'a-index'), target: side('needle one\n', 'a-index'), untracked: false },
				{ path: 'b.ts', language: 'typescript', base: side('old\n', 'b-base'), index: side('needle two\n', 'b-index'), target: side('needle two\n', 'b-index'), untracked: false },
			],
		});
		mockChangesAndFingerprint(changeSet);

		const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
			if (this.classList.contains('pane-content')) return rect({ top: 0, height: 1000 });
			if (this.classList.contains('file-header') && fileSectionIndexOf(this) === 1) return rect({ top: 400, height: 32 });
			return rect({ top: 0, height: 0 });
		});

		try {
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelectorAll('.diff-file')).toHaveLength(2));
			// Both files are staged, so both start collapsed.
			expect(container.querySelectorAll('.diff-root')).toHaveLength(0);

			await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
			const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;
			await fireEvent.input(input, { target: { value: 'needle' } });
			await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 2'));
			// Still collapsed: the current match is file a, not file b.
			expect(container.querySelectorAll('.diff-root')).toHaveLength(1);

			const matchMarks = Array.from(container.querySelectorAll('.ruler-mark.kind-match')) as HTMLElement[];
			expect(matchMarks).toHaveLength(1);
			// Header top 400 / content height 1000 * 1000px track = 400px.
			expect(parseFloat(matchMarks[0].style.top)).toBeCloseTo(400, 0);
		} finally {
			rectSpy.mockRestore();
		}
	});

	test('a match inside a collapsed region anchors at the .collapsed element', async () => {
		stubAnimationFrame();
		stubTrackHeight(1000);

		// Two needles: the first (outside any fold) becomes the
		// auto-selected CURRENT match and is irrelevant here. The second
		// sits inside a collapsed context run and is never navigated to,
		// so it stays genuinely folded for this assertion (navigating to
		// it would expand it — a separate, pre-existing feature, not what
		// this test is about).
		// 'needle one' replaces the first line (a CHANGED line, which never
		// folds) so it is never hidden; 'needle two' sits in the middle of
		// ten otherwise-identical lines, which collapses (more than 2
		// lines would be hidden either side of the kept 3-line context).
		const sameLines = Array.from({ length: 10 }, (_, index) => (index === 5 ? 'needle two' : `same ${index}`));
		const oldContent = ['old start', ...sameLines, 'old end'].join('\n');
		const newContent = ['needle one', ...sameLines, 'new end'].join('\n');
		const changeSet = changeSetFixture({
			files: [{ path: 'src/lib.rs', language: 'rust', base: side(oldContent, 'base-blob'), index: side(oldContent, 'base-blob'), target: side(newContent, 'target-blob'), untracked: false }],
		});
		mockChangesAndFingerprint(changeSet);

		const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
			if (this.classList.contains('pane-content')) return rect({ top: 0, height: 1000 });
			if (this.classList.contains('collapsed')) return rect({ top: 300, height: 26 });
			return rect({ top: 0, height: 0 });
		});

		try {
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());
			// The second needle sits inside a collapsed context run.
			expect(container.querySelector('.collapsed')).not.toBeNull();
			expect(container.textContent).not.toContain('needle two');

			await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
			const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;
			await fireEvent.input(input, { target: { value: 'needle' } });
			await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 2'));
			// Still folded: the current match is "needle one", not "needle two".
			expect(container.querySelector('.collapsed')).not.toBeNull();

			const matchMarks = Array.from(container.querySelectorAll('.ruler-mark.kind-match')) as HTMLElement[];
			expect(matchMarks).toHaveLength(1);
			// `.collapsed` top 300 / content height 1000 * 1000px track = 300px.
			expect(parseFloat(matchMarks[0].style.top)).toBeCloseTo(300, 0);
		} finally {
			rectSpy.mockRestore();
		}
	});

	test('changing only currentMatchIndex does not re-measure', async () => {
		stubAnimationFrame();
		stubTrackHeight(1000);

		const changeSet = changeSetFixture({
			files: [{ path: 'src/lib.rs', language: 'rust', base: side('old\n', 'base-blob'), index: side('old\n', 'base-blob'), target: side('needle one\nneedle two\n', 'target-blob'), untracked: false }],
		});
		mockChangesAndFingerprint(changeSet);

		const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
			if (this.classList.contains('pane-content')) return rect({ top: 0, height: 1000 });
			return rect({ top: 100, height: 20 });
		});

		try {
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

			await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
			const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;
			await fireEvent.input(input, { target: { value: 'needle' } });
			await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 2'));

			const contentElement = container.querySelector('.pane-content') as HTMLElement;
			const measureSpy = vi.spyOn(contentElement, 'getBoundingClientRect');
			measureSpy.mockClear();

			// Advances to the next match: only `currentMatchIndex` changes
			// (the query, and therefore `globalMatches`, is unchanged).
			await fireEvent.keyDown(input, { key: 'Enter' });
			await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('2 of 2'));

			expect(measureSpy).not.toHaveBeenCalled();
		} finally {
			rectSpy.mockRestore();
		}
	});

	test('clicking a change mark scrolls its run into view', async () => {
		stubAnimationFrame();
		stubTrackHeight(1000);

		const changeSet = changeSetFixture();
		mockChangesAndFingerprint(changeSet);

		const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement): DOMRect {
			if (this.classList.contains('pane-content')) return rect({ top: 0, height: 1000 });
			if (this.classList.contains('overview-ruler')) return rect({ top: 0, height: 1000 });
			return rect({ top: 500, height: 20 });
		});

		try {
			const { container } = render(DiffView, { props: { source: { kind: 'git', spec: { repository: '/repository' } } } });
			await vi.waitFor(() => expect(container.querySelector('.diff-file')).not.toBeNull());

			const removedRow = container.querySelector('.row.removed') as HTMLElement;
			expect(removedRow).not.toBeNull();
			const scrollSpy = vi.spyOn(removedRow, 'scrollIntoView');

			const mixedMark = container.querySelector('.ruler-mark.kind-mixed') as HTMLElement;
			expect(mixedMark).not.toBeNull();
			const ruler = container.querySelector('.overview-ruler') as HTMLElement;
			const markTop = parseFloat(mixedMark.style.top);

			await fireEvent.click(ruler, { clientY: markTop + 1 });
			await vi.waitFor(() => expect(scrollSpy).toHaveBeenCalledWith({ block: 'center' }));
		} finally {
			rectSpy.mockRestore();
		}
	});
});
