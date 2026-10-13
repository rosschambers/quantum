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
		const headings = Array.from(container.querySelectorAll('.group-heading .group-label')).map((element) => `${element.textContent?.replace(/[\u25B6\u25BC]/g, '').trim()} ${element.nextElementSibling?.textContent}`);
		expect(headings.some((text) => text?.startsWith('Unstaged') && text.includes('2'))).toBe(true);
		expect(headings.some((text) => text?.startsWith('Staged') && text.includes('1'))).toBe(true);
	});

	test('clicking a group heading collapses and expands that group only', async () => {
		const entries: ReviewEntry[] = [
			entry({ path: 'src/a.ts', section: 'unstaged' }),
			entry({ path: 'src/b.ts', section: 'unstaged' }),
			entry({ path: 'src/c.ts', section: 'staged' }),
		];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		const unstagedToggle = container.querySelector<HTMLButtonElement>('.group-heading button[data-group="unstaged"]');
		expect(unstagedToggle?.getAttribute('aria-expanded')).toBe('true');
		expect(container.querySelectorAll('.entry')).toHaveLength(3);

		await fireEvent.click(unstagedToggle!);
		expect(unstagedToggle?.getAttribute('aria-expanded')).toBe('false');
		const visibleNames = Array.from(container.querySelectorAll('.entry .filename')).map((element) => element.textContent);
		expect(visibleNames).toEqual(['c.ts']);
		// The heading keeps its count while collapsed.
		expect(unstagedToggle?.textContent).toContain('2');

		await fireEvent.click(unstagedToggle!);
		expect(container.querySelectorAll('.entry')).toHaveLength(3);
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

	test('the mini-stats deletions count renders a real minus sign, not a literal backslash escape', () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged' })];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		const miniStats = container.querySelector('.mini-stats') as HTMLElement;
		expect(miniStats.textContent).toContain('\u2212');
		expect(miniStats.textContent).not.toContain('\\u2212');
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

describe('ChangesSidebar tree view', () => {
	const treeEntries = (): ReviewEntry[] => [
		entry({ path: '.github/workflows/runner-watchdog.yml', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/api/calendarQueryKeys.ts', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/realtime/EventsContext.tsx', section: 'unstaged' }),
		entry({ path: 'src/Murmur8.Web/lib/realtime/EventsContext.tsx', section: 'staged' }),
	];

	function folderNames(container: HTMLElement): string[] {
		return Array.from(container.querySelectorAll('.folder-row .folder-name')).map((element) => element.textContent ?? '');
	}

	test('list stays the default: no folder rows without view="tree"', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn() } });
		expect(container.querySelectorAll('.folder-row')).toHaveLength(0);
		expect(container.querySelectorAll('.entry .directory').length).toBeGreaterThan(0);
	});

	test('tree view shows compacted folders per group and one-line file rows', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		expect(folderNames(container)).toEqual(['.github/workflows/', 'src/Murmur8.Web/lib/', 'api/', 'realtime/', 'src/Murmur8.Web/lib/realtime/']);
		expect(container.querySelectorAll('.entry.tree .directory')).toHaveLength(0);
	});

	test('rows indent 16px per depth', () => {
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		const calendarRow = Array.from(container.querySelectorAll('.entry.tree')).find((element) => element.textContent?.includes('calendarQueryKeys.ts'));
		expect(calendarRow?.querySelector('button')?.getAttribute('style')).toContain('padding-left: 32px');
	});

	test('clicking a folder folds it in its own group only, without selecting', async () => {
		const onSelect = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect, view: 'tree' } });
		const unstagedSource = container.querySelector<HTMLButtonElement>('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		await fireEvent.click(unstagedSource!);
		expect(unstagedSource?.getAttribute('aria-expanded')).toBe('false');
		expect(folderNames(container)).toEqual(['.github/workflows/', 'src/Murmur8.Web/lib/', 'src/Murmur8.Web/lib/realtime/']);
		expect(onSelect).not.toHaveBeenCalled();
	});

	test('a folded folder holding the active file is marked active', async () => {
		// Index 1 is calendarQueryKeys.ts (entries are passed in display order).
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 1, onSelect: vi.fn(), view: 'tree' } });
		const folder = container.querySelector<HTMLButtonElement>('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		await fireEvent.click(folder!);
		expect(folder?.closest('.folder-row')?.classList.contains('active')).toBe(true);
	});

	test('the title toggle reflects the view and calls onToggleView', async () => {
		const onToggleView = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), view: 'tree', onToggleView } });
		const toggle = container.querySelector<HTMLButtonElement>('.sidebar-title .view-toggle');
		expect(toggle?.getAttribute('aria-pressed')).toBe('true');
		expect(toggle?.getAttribute('title')).toBe('Show as list (t)');
		await fireEvent.click(toggle!);
		expect(onToggleView).toHaveBeenCalledTimes(1);
	});

	test('the sidebar is 260px wide', () => {
		expect(sidebarSource).toMatch(/\.changes-sidebar\s*\{[^}]*width:\s*260px/);
	});

	test('an Unstaged folder checkbox stages every file below it, in tree order, without selecting', async () => {
		const onStageMany = vi.fn();
		const onSelect = vi.fn();
		const onStage = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect, onStage, onStageMany, view: 'tree' } });
		const folderButton = container.querySelector('.folder-row button[data-group="unstaged"][data-folder="src/Murmur8.Web/lib"]');
		const checkbox = folderButton!.closest('.folder-row')!.querySelector<HTMLInputElement>('.checkbox');
		await fireEvent.click(checkbox!);
		expect(onStageMany).toHaveBeenCalledTimes(1);
		expect(onStageMany.mock.calls[0][0].map((item: ReviewEntry) => item.id)).toEqual([
			'src/Murmur8.Web/lib/api/calendarQueryKeys.ts:unstaged',
			'src/Murmur8.Web/lib/realtime/EventsContext.tsx:unstaged',
		]);
		expect(onSelect).not.toHaveBeenCalled();
		expect(onStage).not.toHaveBeenCalled();
	});

	test('a Staged folder checkbox unstages every file below it', async () => {
		const onUnstageMany = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), onUnstageMany, view: 'tree' } });
		const folderButton = container.querySelector('.folder-row button[data-group="staged"]');
		const checkbox = folderButton!.closest('.folder-row')!.querySelector<HTMLInputElement>('.checkbox');
		await fireEvent.click(checkbox!);
		expect(onUnstageMany.mock.calls[0][0].map((item: ReviewEntry) => item.id)).toEqual(['src/Murmur8.Web/lib/realtime/EventsContext.tsx:staged']);
	});

	test('Space on a folder checkbox stages like a click', async () => {
		const onStageMany = vi.fn();
		const { container } = render(ChangesSidebar, { props: { entries: treeEntries(), stageable: true, activeIndex: 0, onSelect: vi.fn(), onStageMany, view: 'tree' } });
		const checkbox = container.querySelector<HTMLInputElement>('.folder-row .checkbox');
		await fireEvent.keyDown(checkbox!, { key: ' ' });
		expect(onStageMany).toHaveBeenCalledTimes(1);
	});

	test('a non-stageable tree has no checkboxes', () => {
		const entries = treeEntries().filter((item) => item.section === 'unstaged').map((item) => ({ ...item, section: 'none' as const, id: `${item.path}:none` }));
		const { container } = render(ChangesSidebar, { props: { entries, stageable: false, activeIndex: 0, onSelect: vi.fn(), view: 'tree' } });
		expect(container.querySelectorAll('.folder-row')).not.toHaveLength(0);
		expect(container.querySelectorAll('.checkbox')).toHaveLength(0);
	});
});
