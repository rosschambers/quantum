// Sanitizers for HTML that qv builds from file content. Markdown may contain
// raw HTML, and marked passes it through untouched, so everything rendered with
// {@html} or innerHTML goes through here first. Design:
// docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md
import DOMPurify from 'dompurify';
import { escapeHtml } from './highlighter';
import { CODE_COPY_RESERVED_CLASSES } from './codeCopy';

// Scheme allow-list for URI attributes. Every scheme not listed never matches,
// so javascript: and vbscript: are rejected, and so is data: on links. data:
// images still pass through DOMPurify's separate data-URI rule for img and media
// tags. file: is allowed because qv resolves relative markdown images to
// file:// URLs. Relative references and #fragments match the final two alternatives.
const ALLOWED_URI_REGEXP = /^(?:(?:https?|mailto|file):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

// Markdown gets its own DOMPurify instance, so the hook below never reaches sanitizeSvg or any
// other user of the default instance.
const markdownPurifier = DOMPurify(window);

// The header's classes belong to this view. Author markup may not carry them, or a fake block
// could pick up the header's styling. An element keeps its other classes.
markdownPurifier.addHook('afterSanitizeAttributes', (element: Element) => {
    if (!element.hasAttribute('class')) return;
    element.classList.remove(...CODE_COPY_RESERVED_CLASSES);
    if (element.classList.length === 0) {
        element.removeAttribute('class');
    }
});

export function sanitizeMarkdownHtml(html: string): string {
    // Fail closed: without DOMPurify, show the source as text instead of raw HTML.
    if (!markdownPurifier.isSupported) {
        return escapeHtml(html);
    }
    return markdownPurifier.sanitize(html, {
        // A button is a control with no meaning in a document. Its text stays, as plain content.
        FORBID_TAGS: ['style', 'form', 'button'],
        // `name` on images and forms shadows document properties; forbidding it
        // replaces DOMPurify's own clobbering check, which is switched off below.
        // `style` lets content position an overlay over a real code block, so its
        // innocent text could disguise what the Copy button copies. The owner's
        // documents do not use inline styles.
        FORBID_ATTR: ['name', 'style'],
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
