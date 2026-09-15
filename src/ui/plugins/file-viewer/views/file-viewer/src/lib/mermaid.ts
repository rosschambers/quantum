import { escapeHtml } from './highlighter';

/** The client-side renderer that owns a diagram placeholder. */
export type DiagramRenderer = 'mermaid' | 'graphviz';

/**
 * Checks whether a language hint refers to mermaid (case-insensitive)
 * @param language Optional language hint from the markdown renderer
 * @returns True if the language is exactly "mermaid"
 */
export function isMermaidLanguage(language: string | undefined): boolean {
	return language?.toLowerCase() === 'mermaid';
}

/**
 * Checks whether a language hint refers to Graphviz DOT (case-insensitive).
 * Mermaid cannot parse DOT syntax (digraph/graph sources), so those fences
 * get their own renderer (@hpcc-js/wasm-graphviz) instead of mermaid.
 * @param language Optional language hint from the markdown renderer
 * @returns True if the language is dot, graphviz, or digraph
 */
export function isGraphvizLanguage(language: string | undefined): boolean {
	const normalized = language?.toLowerCase();
	return normalized === 'dot' || normalized === 'graphviz' || normalized === 'digraph';
}

/**
 * Renders a diagram code block as an HTML placeholder div, tagged with the
 * renderer that should claim it
 * @param source The raw diagram source of the code block
 * @param renderer Which client-side renderer owns this placeholder
 * @returns HTML string for the client-side renderer to pick up
 */
export function diagramPlaceholderHtml(source: string, renderer: DiagramRenderer): string {
	return `<div class="diagram-block" data-diagram-renderer="${renderer}" data-diagram-status="pending">${escapeHtml(source.trim())}</div>`;
}
