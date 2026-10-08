import { describe, test, expect } from 'vitest';
import { pairChangeSet, type PairFile } from './pairChangeSet';
import { reviewEntries } from './reviewModel';

function file(overrides: Partial<PairFile> = {}): PairFile {
	return { path: '/left.ts', content: 'left content', binary: false, ...overrides };
}

describe('pairChangeSet', () => {
	test('builds a non-stageable change set with one file labelled by both full paths', () => {
		const changeSet = pairChangeSet(file({ path: '/a.ts', content: 'old\n' }), file({ path: '/b.ts', content: 'new\n' }));
		expect(changeSet.stageable).toBe(false);
		expect(changeSet.base_label).toBe('/a.ts');
		expect(changeSet.target_label).toBe('/b.ts');
		expect(changeSet.files).toHaveLength(1);
		expect(changeSet.files[0].path).toBe('/b.ts');
		expect(changeSet.files[0].base?.content).toBe('old\n');
		expect(changeSet.files[0].target?.content).toBe('new\n');
		expect(changeSet.files[0].index).toBeUndefined();
	});

	test('a binary side (image or video) carries no content, only the binary flag', () => {
		const changeSet = pairChangeSet(file({ binary: true, content: '' }), file({ content: 'text' }));
		expect(changeSet.files[0].base?.binary).toBe(true);
		expect(changeSet.files[0].base?.content).toBeUndefined();
		expect(changeSet.files[0].target?.binary).toBe(false);
		expect(changeSet.files[0].target?.content).toBe('text');
	});

	test('falls back to the left side\'s language when the right side has none', () => {
		const changeSet = pairChangeSet(file({ language: 'rust' }), file({ language: undefined }));
		expect(changeSet.files[0].language).toBe('rust');
	});

	test('the right side\'s language wins when both sides declare one', () => {
		const changeSet = pairChangeSet(file({ language: 'rust' }), file({ language: 'typescript' }));
		expect(changeSet.files[0].language).toBe('typescript');
	});

	test('feeding the result through reviewEntries yields exactly one, non-stageable, status M entry', () => {
		const changeSet = pairChangeSet(file({ path: '/a.ts' }), file({ path: '/b.ts' }));
		const entries = reviewEntries(changeSet);
		expect(entries).toHaveLength(1);
		expect(entries[0].section).toBe('none');
		expect(entries[0].status).toBe('M');
		expect(entries[0].path).toBe('/b.ts');
	});
});
