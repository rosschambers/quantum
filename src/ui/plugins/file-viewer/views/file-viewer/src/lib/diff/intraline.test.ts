import { describe, test, expect } from 'vitest';
import { intraLine } from './intraline';

describe('intraLine', () => {
	test('marks only the changed identifiers in the playground Rust example', () => {
		const result = intraLine(
			'let ext_lower = extension.to_lowercase();',
			'let extension_lower = extension.to_ascii_lowercase();',
		);
		expect(result).not.toBeNull();
		const oldText = 'let ext_lower = extension.to_lowercase();';
		const newText = 'let extension_lower = extension.to_ascii_lowercase();';
		const removedText = result!.removed.map(([start, end]) => oldText.slice(start, end));
		const addedText = result!.added.map(([start, end]) => newText.slice(start, end));
		expect(removedText).toEqual(['ext_lower', 'to_lowercase']);
		expect(addedText).toEqual(['extension_lower', 'to_ascii_lowercase']);
	});

	test('identical lines produce no ranges but are not null', () => {
		const result = intraLine('same line', 'same line');
		expect(result).toEqual({ removed: [], added: [] });
	});

	test('two separate changed words produce two separate, non-adjacent ranges', () => {
		const result = intraLine('alpha middle beta', 'ALPHA middle BETA');
		expect(result).not.toBeNull();
		expect(result!.removed).toEqual([
			[0, 5],
			[13, 17],
		]);
		expect(result!.added).toEqual([
			[0, 5],
			[13, 17],
		]);
	});

	test('a single word substitution stays one range, not two touching ones', () => {
		const result = intraLine('the cat sat', 'the bat sat');
		expect(result).toEqual({ removed: [[4, 7]], added: [[4, 7]] });
	});

	test('dissimilar lines return null', () => {
		const result = intraLine(
			'pub fn viewer_file_type_for_extension(extension: &str) -> ViewerFileType {',
			'use serde::{Deserialize, Serialize};',
		);
		expect(result).toBeNull();
	});

	test('pure addition keeps full similarity and returns ranges', () => {
		const result = intraLine('value', 'value plus more');
		expect(result).not.toBeNull();
		expect(result!.removed).toEqual([]);
		expect(result!.added.length).toBeGreaterThan(0);
	});
});
