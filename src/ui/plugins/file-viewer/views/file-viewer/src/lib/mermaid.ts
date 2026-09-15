import { escapeHtml } from './highlighter';

/**
 * Checks whether a language hint refers to mermaid (case-insensitive)
 * @param language Optional language hint from the markdown renderer
 * @returns True if the language is exactly "mermaid"
 */
export function isMermaidLanguage(language: string | undefined): boolean {
	return language?.toLowerCase() === 'mermaid';
}

/**
 * Renders a mermaid code block as an HTML placeholder div
 * @param source The raw mermaid source of the code block
 * @returns HTML string for the client-side renderer to pick up
 */
export function mermaidPlaceholderHtml(source: string): string {
	return `<div class="mermaid-block" data-mermaid-status="pending">${escapeHtml(source.trim())}</div>`;
}
