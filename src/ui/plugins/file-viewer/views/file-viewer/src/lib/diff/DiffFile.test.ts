import { describe, test, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import DiffFile from './DiffFile.svelte';
import type { ReviewEntry } from './reviewModel';
import type { FileSide } from '@quantum/client';

afterEach(() => {
	cleanup();
});

function side(content: string, overrides: Partial<FileSide> = {}): FileSide {
	return { content, blob: 'blob', mode: '100644', binary: false, too_large: false, ...overrides };
}

function entry(overrides: Partial<ReviewEntry> = {}): ReviewEntry {
	return {
		id: 'src/lib.rs:unstaged',
		fileId: 'src/lib.rs',
		path: 'src/lib.rs',
		status: 'M',
		section: 'unstaged',
		partiallyStaged: false,
		oldSide: side('old line'),
		newSide: side('new line'),
		...overrides,
	};
}

describe('DiffFile', () => {
	test('a binary file shows the binary placeholder', () => {
		const { container } = render(DiffFile, {
			props: {
				entry: entry({ oldSide: side('', { content: undefined, binary: true }), newSide: side('', { content: undefined, binary: true }) }),
				stageable: false,
				layout: 'unified',
			},
		});
		expect(container.textContent).toContain('Binary file changed');
	});

	test('a too-large file shows the too-large placeholder', () => {
		const { container } = render(DiffFile, {
			props: {
				entry: entry({ oldSide: side('', { content: undefined, too_large: true }), newSide: side('', { content: undefined, too_large: true }) }),
				stageable: false,
				layout: 'unified',
			},
		});
		expect(container.textContent).toContain('Too large to diff');
	});

	test('a diff with more than 1000 changed lines shows the large-diff placeholder with a Load diff button, which renders the diff when clicked', async () => {
		const oldLines = Array.from({ length: 600 }, (_, index) => `old ${index}`).join('\n');
		const newLines = Array.from({ length: 600 }, (_, index) => `new ${index}`).join('\n');
		const { container } = render(DiffFile, {
			props: {
				entry: entry({ oldSide: side(oldLines), newSide: side(newLines) }),
				stageable: false,
				layout: 'unified',
			},
		});
		expect(container.textContent).toContain('Large diff:');
		expect(container.textContent).toContain('not rendered yet.');
		const loadButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes('Load diff'));
		expect(loadButton).toBeDefined();
		await fireEvent.click(loadButton!);
		expect(container.textContent).not.toContain('Large diff:');
		expect(container.querySelector('.diff-root')).not.toBeNull();
	});

	test('the stage button reads "Stage" for an unstaged entry and "\u2713 Staged" for a staged entry, and is absent when not stageable', async () => {
		const onStage = vi.fn();
		const { container, rerender } = render(DiffFile, {
			props: { entry: entry({ section: 'unstaged' }), stageable: true, layout: 'unified', onStage },
		});
		const stageButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.trim() === 'Stage');
		expect(stageButton).toBeDefined();
		await fireEvent.click(stageButton!);
		expect(onStage).toHaveBeenCalledWith(expect.objectContaining({ fileId: 'src/lib.rs' }));

		const onUnstage = vi.fn();
		await rerender({ entry: entry({ section: 'staged', id: 'src/lib.rs:staged' }), stageable: true, layout: 'unified', onUnstage });
		const stagedButton = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes('Staged'));
		expect(stagedButton?.textContent).toContain('\u2713 Staged');
		await fireEvent.click(stagedButton!);
		expect(onUnstage).toHaveBeenCalled();

		await rerender({ entry: entry({ section: 'unstaged' }), stageable: false, layout: 'unified' });
		const anyStageButton = Array.from(container.querySelectorAll('button')).find(
			(button) => button.textContent?.trim() === 'Stage' || button.textContent?.includes('Staged'),
		);
		expect(anyStageButton).toBeUndefined();
	});

	test('a staged entry is collapsed by default; an unstaged entry is not', () => {
		const staged = render(DiffFile, { props: { entry: entry({ section: 'staged' }), stageable: true, layout: 'unified' } });
		expect(staged.container.querySelector('.diff-root')).toBeNull();
		staged.unmount();

		const unstaged = render(DiffFile, { props: { entry: entry({ section: 'unstaged' }), stageable: true, layout: 'unified' } });
		expect(unstaged.container.querySelector('.diff-root')).not.toBeNull();
		unstaged.unmount();
	});

	test('clicking the header calls onToggleCollapsed and an explicit collapsed prop overrides the default', async () => {
		const onToggleCollapsed = vi.fn();
		const { container } = render(DiffFile, {
			props: { entry: entry({ section: 'staged' }), stageable: true, layout: 'unified', collapsed: false, onToggleCollapsed },
		});
		// Explicit collapsed=false overrides the staged default of collapsed.
		expect(container.querySelector('.diff-root')).not.toBeNull();

		const header = container.querySelector('.file-header') as HTMLElement;
		await fireEvent.click(header);
		expect(onToggleCollapsed).toHaveBeenCalledTimes(1);
	});

	test('a rename in the same directory shows "oldName \u2192 newName"', () => {
		const { container } = render(DiffFile, {
			props: {
				entry: entry({ path: 'src/lib/newName.ts', oldPath: 'src/lib/oldName.ts', status: 'R' }),
				stageable: false,
				layout: 'unified',
			},
		});
		const header = container.querySelector('.file-header') as HTMLElement;
		expect(header.textContent).toContain('oldName.ts \u2192 ');
		expect(header.textContent).toContain('newName.ts');
	});

	test('a rename across directories shows the full old path and the new path', () => {
		const { container } = render(DiffFile, {
			props: {
				entry: entry({ path: 'src/new/place.ts', oldPath: 'src/old/place.ts', status: 'R' }),
				stageable: false,
				layout: 'unified',
			},
		});
		const header = container.querySelector('.file-header') as HTMLElement;
		expect(header.textContent).toContain('src/old/');
		expect(header.textContent).toContain('place.ts \u2192 ');
		expect(header.textContent).toContain('src/new/');
	});

	test('shows the partially staged pill when the entry is partially staged', () => {
		const { container } = render(DiffFile, {
			props: { entry: entry({ partiallyStaged: true }), stageable: true, layout: 'unified' },
		});
		expect(container.textContent).toContain('partially staged');
	});

	test('an externally controlled expandedKeys prop is the source of truth, and toggling reports the change via onExpandedKeysChange instead of expanding internally', async () => {
		const sameLines = Array.from({ length: 10 }, (_, index) => `same ${index}`);
		const oldContent = ['old start', ...sameLines, 'old end'].join('\n');
		const newContent = ['new start', ...sameLines, 'new end'].join('\n');
		const onExpandedKeysChange = vi.fn();
		const { container, rerender } = render(DiffFile, {
			props: {
				entry: entry({ oldSide: side(oldContent), newSide: side(newContent) }),
				stageable: false,
				layout: 'unified',
				expandedKeys: new Set(),
				onExpandedKeysChange,
			},
		});
		const collapsedRow = container.querySelector('.collapsed') as HTMLElement;
		expect(collapsedRow).not.toBeNull();

		await fireEvent.click(collapsedRow);
		expect(onExpandedKeysChange).toHaveBeenCalledTimes(1);
		const [reportedKeys] = onExpandedKeysChange.mock.calls[0];
		expect(reportedKeys.size).toBe(1);
		// Controlled mode: the component must not expand on its own — the
		// prop (still an empty set) remains the source of truth, so the row
		// stays collapsed until the caller re-renders with the new keys.
		expect(container.querySelector('.collapsed')).not.toBeNull();

		await rerender({
			entry: entry({ oldSide: side(oldContent), newSide: side(newContent) }),
			stageable: false,
			layout: 'unified',
			expandedKeys: reportedKeys,
			onExpandedKeysChange,
		});
		expect(container.querySelector('.collapsed')).toBeNull();
		expect(container.querySelector('.recollapse')).not.toBeNull();
	});

	test('expandedKeys is uncontrolled (manages its own state) when the prop is omitted, exactly as before', async () => {
		const sameLines = Array.from({ length: 10 }, (_, index) => `same ${index}`);
		const oldContent = ['old start', ...sameLines, 'old end'].join('\n');
		const newContent = ['new start', ...sameLines, 'new end'].join('\n');
		const { container } = render(DiffFile, {
			props: { entry: entry({ oldSide: side(oldContent), newSide: side(newContent) }), stageable: false, layout: 'unified' },
		});
		const collapsedRow = container.querySelector('.collapsed') as HTMLElement;
		expect(collapsedRow).not.toBeNull();
		await fireEvent.click(collapsedRow);
		expect(container.querySelector('.collapsed')).toBeNull();
		expect(container.querySelector('.recollapse')).not.toBeNull();
	});
});
