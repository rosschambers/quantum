import { describe, it, expect } from 'vitest';
import { isMermaidLanguage, mermaidPlaceholderHtml } from './mermaid';

describe('isMermaidLanguage', () => {
	it('matches mermaid case-insensitively', () => {
		expect(isMermaidLanguage('mermaid')).toBe(true);
		expect(isMermaidLanguage('Mermaid')).toBe(true);
	});

	it('does not match other languages or undefined', () => {
		expect(isMermaidLanguage('javascript')).toBe(false);
		expect(isMermaidLanguage(undefined)).toBe(false);
		expect(isMermaidLanguage('mermaid-js')).toBe(false);
	});
});

describe('mermaidPlaceholderHtml', () => {
	it('wraps the source in a mermaid-block div', () => {
		expect(mermaidPlaceholderHtml('graph TD;\n  A --> B;')).toBe(
			'<div class="mermaid-block" data-mermaid-status="pending">graph TD;\n  A --&gt; B;</div>',
		);
	});

	it('escapes HTML-significant characters in the source', () => {
		expect(mermaidPlaceholderHtml('<script>alert("x")</script>')).toContain('&lt;script&gt;');
	});
});
