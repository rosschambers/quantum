// Sanitizers for HTML that qv builds from file content. Markdown may contain
// raw HTML, and marked passes it through untouched, so everything rendered with
// {@html} or innerHTML goes through here first. Design:
// docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md
import DOMPurify from 'dompurify';

// DOMPurify's default list without the exotic schemes, plus file: (qv resolves
// relative markdown images to file://). Relative references and #fragments
// still pass through the final two alternatives. javascript: and vbscript: never match.
const ALLOWED_URI_REGEXP = /^(?:(?:https?|mailto|file):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

export function sanitizeMarkdownHtml(html: string): string {
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
    return DOMPurify.sanitize(svg, {
        USE_PROFILES: { svg: true, svgFilters: true },
        ALLOWED_URI_REGEXP,
    });
}
