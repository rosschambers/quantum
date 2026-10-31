// Sanitizers for HTML that qv builds from file content. Markdown may contain
// raw HTML, and marked passes it through untouched, so everything rendered with
// {@html} or innerHTML goes through here first. Design:
// docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md
import DOMPurify from 'dompurify';
import { escapeHtml } from './highlighter';

// Scheme allow-list for URI attributes. Every scheme not listed never matches,
// so javascript: and vbscript: are rejected, and so is data: on links. data:
// images still pass through DOMPurify's separate data-URI rule for img and media
// tags. file: is allowed because qv resolves relative markdown images to
// file:// URLs. Relative references and #fragments match the final two alternatives.
const ALLOWED_URI_REGEXP = /^(?:(?:https?|mailto|file):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

export function sanitizeMarkdownHtml(html: string): string {
    // Fail closed: without DOMPurify, show the source as text instead of raw HTML.
    if (!DOMPurify.isSupported) {
        return escapeHtml(html);
    }
    return DOMPurify.sanitize(html, {
        FORBID_TAGS: ['style', 'form'],
        // `name` on images and forms shadows document properties; forbidding it
        // replaces DOMPurify's own clobbering check, which is switched off below.
        FORBID_ATTR: ['name'],
        ALLOWED_URI_REGEXP,
        // On, DOMPurify strips heading ids such as "images" or "title" because
        // they collide with document properties, which breaks table-of-contents anchors.
        SANITIZE_DOM: false,
    });
}

export function sanitizeSvg(svg: string): string {
    if (!DOMPurify.isSupported) {
        return escapeHtml(svg);
    }
    return DOMPurify.sanitize(svg, {
        USE_PROFILES: { svg: true, svgFilters: true },
        // The SVG profile allows style, and a stylesheet inside a diagram can restyle the whole page.
        FORBID_TAGS: ['style'],
        ALLOWED_URI_REGEXP,
    });
}
