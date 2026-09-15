import { describe, it, expect } from 'vitest';
import { isMermaidLanguage, isGraphvizLanguage, diagramPlaceholderHtml } from './mermaid';

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

describe('isGraphvizLanguage', () => {
	it('matches the graphviz language family case-insensitively', () => {
	    expect(isGraphvizLanguage('dot')).toBe(true);
	    expect(isGraphvizLanguage('DOT')).toBe(true);
	    expect(isGraphvizLanguage('graphviz')).toBe(true);
	    expect(isGraphvizLanguage('digraph')).toBe(true);
	});

	it('does not match mermaid or other languages', () => {
	    expect(isGraphvizLanguage('mermaid')).toBe(false);
	    expect(isGraphvizLanguage('graph')).toBe(false);
	    expect(isGraphvizLanguage(undefined)).toBe(false);
	});
});

describe('diagramPlaceholderHtml', () => {
	it('wraps the source in a diagram-block div tagged with the renderer', () => {
	    expect(diagramPlaceholderHtml('graph TD;\n  A --> B;', 'mermaid')).toBe(
	        '<div class="diagram-block" data-diagram-renderer="mermaid" data-diagram-status="pending">graph TD;\n  A --&gt; B;</div>',
	    );
	    expect(diagramPlaceholderHtml('digraph g {\n  a -> b;\n}', 'graphviz')).toBe(
	        '<div class="diagram-block" data-diagram-renderer="graphviz" data-diagram-status="pending">digraph g {\n  a -&gt; b;\n}</div>',
	    );
	});

	it('escapes HTML-significant characters in the source', () => {
	    expect(diagramPlaceholderHtml('<script>alert("x")</script>', 'mermaid')).toContain('&lt;script&gt;');
	});

	it('trims leading and trailing whitespace from the source', () => {
	    expect(diagramPlaceholderHtml('\n  graph TD;\n    A --> B;\n  ', 'mermaid')).toBe(
	        '<div class="diagram-block" data-diagram-renderer="mermaid" data-diagram-status="pending">graph TD;\n    A --&gt; B;</div>',
	    );
	});
});
