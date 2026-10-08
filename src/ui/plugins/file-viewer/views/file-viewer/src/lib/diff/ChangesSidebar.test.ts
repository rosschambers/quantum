import { describe, test, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import ChangesSidebar from './ChangesSidebar.svelte';
import type { ReviewEntry } from './reviewModel';
import type { FileSide } from '@quantum/client';
import { clearLineDiffCache } from './lineDiffCache';
import sidebarSource from './ChangesSidebar.svelte?raw';

afterEach(() => {
	cleanup();
	clearLineDiffCache();
	vi.restoreAllMocks();
});

/**
 * Git blob ids are content-addressed: same content, same blob; different
 * content, different blob. Reusing one literal blob id across distinct
 * contents (as this fixture used to) silently defeats the blob-keyed line
 * diff cache `ChangesSidebar.svelte` now reads from.
 */
function blobFor(content: string): string {
	let hash = 0;
	for (let index = 0; index < content.length; index++) {
		hash = (hash * 31 + content.charCodeAt(index)) | 0;
	}
	return `blob-${content.length}-${hash}`;
}

function side(content: string): FileSide {
	return { content, blob: blobFor(content), mode: '100644', binary: false, too_large: false };
}

function entry(overrides: Partial<ReviewEntry>): ReviewEntry {
	return {
		id: `${overrides.path ?? 'file.ts'}:${overrides.section ?? 'unstaged'}`,
		fileId: overrides.path ?? 'file.ts',
		path: 'file.ts',
		status: 'M',
		section: 'unstaged',
		partiallyStaged: false,
		oldSide: side('old'),
		newSide: side('new'),
		...overrides,
	};
}

describe('ChangesSidebar', () => {
	test('stageable sets group entries under Unstaged N and Staged N headings', () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', section: 'unstaged' }),
			entry({ path: 'src/b.ts', section: 'unstaged' }),
			entry({ path: 'src/c.ts', section: 'staged' }),
		];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		const headings = Array.from(container.querySelectorAll('.group-heading')).map((element) => element.textContent?.trim());
		expect(headings.some((text) => text?.startsWith('Unstaged') && text.includes('2'))).toBe(true);
		expect(headings.some((text) => text?.startsWith('Staged') && text.includes('1'))).toBe(true);
	});

	test('a non-stageable set renders a flat list with no group headings and no checkboxes', () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', section: 'none' }),
			entry({ path: 'src/b.ts', section: 'none' }),
		];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: false, activeIndex: 0, onSelect: vi.fn() },
		});
		expect(container.querySelectorAll('.group-heading')).toHaveLength(0);
		expect(container.querySelectorAll('.checkbox')).toHaveLength(0);
		expect(container.querySelectorAll('.entry')).toHaveLength(2);
	});

	test('clicking an entry calls onSelect with its index into the original entries array', async () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', section: 'unstaged' }),
			entry({ path: 'src/b.ts', section: 'staged' }),
		];
		const onSelect = vi.fn();
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect },
		});
		const buttons = container.querySelectorAll('.entry button');
		await fireEvent.click(buttons[1]);
		expect(onSelect).toHaveBeenCalledWith(1);
	});

	test('clicking the checkbox stages/unstages without triggering onSelect', async () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged' })];
		const onSelect = vi.fn();
		const onStage = vi.fn();
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect, onStage },
		});
		const checkbox = container.querySelector('.checkbox') as HTMLElement;
		expect(checkbox).not.toBeNull();
		await fireEvent.click(checkbox);
		expect(onSelect).not.toHaveBeenCalled();
		expect(onStage).toHaveBeenCalledWith(entries[0]);
	});

	test('the stage checkbox is a sibling control of the select button, not nested inside it', () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged' })];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		const entryItem = container.querySelector('.entry') as HTMLElement;
		const selectButton = entryItem.querySelector('button') as HTMLElement;
		const checkbox = entryItem.querySelector('.checkbox') as HTMLElement;
		expect(selectButton).not.toBeNull();
		expect(checkbox).not.toBeNull();
		// A `<button>` containing another interactive control is invalid,
		// inaccessible markup — the checkbox must live beside the button
		// inside the `<li>`, never inside it.
		expect(selectButton.contains(checkbox)).toBe(false);
		expect(entryItem.contains(checkbox)).toBe(true);
	});

	test('clicking the checkbox still stages/unstages and does not select, now that it is a sibling of the button', async () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'staged' })];
		const onSelect = vi.fn();
		const onUnstage = vi.fn();
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect, onUnstage },
		});
		const checkbox = container.querySelector('.checkbox') as HTMLElement;
		await fireEvent.click(checkbox);
		expect(onSelect).not.toHaveBeenCalled();
		expect(onUnstage).toHaveBeenCalledWith(entries[0]);
	});

	test('a staged entry checkbox is checked and clicking it unstages', async () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'staged' })];
		const onUnstage = vi.fn();
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn(), onUnstage },
		});
		const checkbox = container.querySelector('.checkbox') as HTMLElement;
		expect(checkbox.classList.contains('checked')).toBe(true);
		await fireEvent.click(checkbox);
		expect(onUnstage).toHaveBeenCalledWith(entries[0]);
	});

	test('the active entry (by activeIndex) carries the active class, others do not', () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', section: 'unstaged' }),
			entry({ path: 'src/b.ts', section: 'unstaged' }),
		];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 1, onSelect: vi.fn() },
		});
		const items = container.querySelectorAll('.entry');
		expect(items[0].classList.contains('active')).toBe(false);
		expect(items[1].classList.contains('active')).toBe(true);
	});

	test('a rename shows "renamed from <old name>" instead of the directory', () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/new.ts', oldPath: 'src/old.ts', status: 'R', section: 'unstaged' })];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		expect(container.textContent).toContain('renamed from old.ts');
	});

	test('the "renamed from" label reads left-to-right even though it inherits .directory\'s rtl ellipsis', () => {
		// Svelte's component-scoped CSS is not injected into jsdom by this
		// project's test setup, so (matching `CodeRenderer.test.ts`'s
		// `scrolls the gutter together with the code` test) the component's
		// own raw `<style>` block is loaded unscoped for the duration of
		// this one test, to assert against the real cascade.
		const stylesheet = document.createElement('style');
		stylesheet.textContent = sidebarSource.split('<style>')[1].split('</style>')[0];
		document.head.appendChild(stylesheet);
		try {
			const entries: ReviewEntry[] = [entry({ path: 'src/new.ts', oldPath: 'src/old.ts', status: 'R', section: 'unstaged' })];
			const { container } = render(ChangesSidebar, {
				props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
			});
			const renameLabel = Array.from(container.querySelectorAll('.directory')).find((element) =>
				element.textContent?.includes('renamed from'),
			) as HTMLElement;
			expect(renameLabel).toBeDefined();
			expect(getComputedStyle(renameLabel).direction).toBe('ltr');
		} finally {
			stylesheet.remove();
		}
	});

	test('a partially staged entry shows a partial dot', () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged', partiallyStaged: true })];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		expect(container.querySelector('.dot')).not.toBeNull();
	});

	test('re-rendering with new entry objects carrying the same blobs does not recompute the line diff', async () => {
		const lineDiffModule = await import('./lineDiff');
		const lineDiffSpy = vi.spyOn(lineDiffModule, 'lineDiff');
		const first: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged' })];
		const { rerender } = render(ChangesSidebar, {
			props: { entries: first, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		expect(lineDiffSpy).toHaveBeenCalledTimes(1);

		const second: ReviewEntry[] = [
			{ ...first[0], oldSide: { ...first[0].oldSide! }, newSide: { ...first[0].newSide! } },
		];
		await rerender({ entries: second, stageable: true, activeIndex: 0, onSelect: vi.fn() });
		expect(lineDiffSpy).toHaveBeenCalledTimes(1);
	});

	test('the title shows the distinct file count', () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', fileId: 'src/a.ts', section: 'unstaged' }),
			entry({ path: 'src/a.ts', fileId: 'src/a.ts', section: 'staged' }),
			entry({ path: 'src/b.ts', fileId: 'src/b.ts', section: 'unstaged' }),
		];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		const title = container.querySelector('.sidebar-title');
		expect(title?.textContent).toContain('2');
	});
});
