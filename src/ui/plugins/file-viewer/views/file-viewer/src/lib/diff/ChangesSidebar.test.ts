import { describe, test, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import ChangesSidebar from './ChangesSidebar.svelte';
import type { ReviewEntry } from './reviewModel';
import type { FileSide } from '@quantum/client';

afterEach(() => {
	cleanup();
});

function side(content: string): FileSide {
	return { content, blob: 'blob', mode: '100644', binary: false, too_large: false };
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

	test('a partially staged entry shows a partial dot', () => {
		const entries: ReviewEntry[] = [entry({ path: 'src/a.ts', section: 'unstaged', partiallyStaged: true })];
		const { container } = render(ChangesSidebar, {
			props: { entries, stageable: true, activeIndex: 0, onSelect: vi.fn() },
		});
		expect(container.querySelector('.dot')).not.toBeNull();
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
