import { describe, it, expect } from 'vitest';
import { sanitizeMarkdownHtml, sanitizeSvg } from './sanitize';

function parse(html: string): HTMLElement {
    const root = document.createElement('div');
    root.innerHTML = html;
    return root;
}

describe('sanitizeMarkdownHtml', () => {
    it('removes script, style and form elements', () => {
        const root = parse(sanitizeMarkdownHtml('<p>a</p><script>alert(1)</script><style>p{}</style><form><input></form>'));
        expect(root.querySelector('script, style, form')).toBeNull();
        expect(root.querySelector('p')?.textContent).toBe('a');
    });

    it('removes every event handler attribute', () => {
        const root = parse(sanitizeMarkdownHtml('<img src="x" onerror="alert(1)"><svg onload="alert(2)"></svg><details ontoggle="alert(3)" open><summary>s</summary></details>'));
        for (const element of Array.from(root.querySelectorAll('*'))) {
            for (const attribute of Array.from(element.attributes)) {
                expect(attribute.name.startsWith('on')).toBe(false);
            }
        }
    });

    it('removes javascript: links but keeps safe and relative ones', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<a id="bad" href="javascript:alert(1)">x</a>' +
            '<a id="https" href="https://example.com">x</a>' +
            '<a id="mail" href="mailto:a@b.c">x</a>' +
            '<a id="relative" href="./other.md">x</a>' +
            '<a id="fragment" href="#task-2">x</a>',
        ));
        expect(root.querySelector('#bad')?.getAttribute('href')).toBeNull();
        expect(root.querySelector('#https')?.getAttribute('href')).toBe('https://example.com');
        expect(root.querySelector('#mail')?.getAttribute('href')).toBe('mailto:a@b.c');
        expect(root.querySelector('#relative')?.getAttribute('href')).toBe('./other.md');
        expect(root.querySelector('#fragment')?.getAttribute('href')).toBe('#task-2');
    });

    it('keeps file: and data: image sources (qv resolves relative images to file://)', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<img id="file" src="file:///home/ixiri/a.png"><img id="data" src="data:image/png;base64,AAAA">',
        ));
        expect(root.querySelector('#file')?.getAttribute('src')).toBe('file:///home/ixiri/a.png');
        expect(root.querySelector('#data')?.getAttribute('src')).toBe('data:image/png;base64,AAAA');
    });

    it('keeps heading ids that shadow document properties, so table-of-contents anchors work', () => {
        const root = parse(sanitizeMarkdownHtml('<h2 id="images">Images</h2><h2 id="title">Title</h2>'));
        expect(root.querySelector('h2')?.id).toBe('images');
        expect(root.querySelectorAll('h2')[1]?.id).toBe('title');
    });

    it('removes name attributes so content cannot shadow document methods', () => {
        const root = parse(sanitizeMarkdownHtml('<img name="getElementById" src="a.png">'));
        expect(root.querySelector('img')?.hasAttribute('name')).toBe(false);
    });

    it('keeps the harmless raw HTML that Ross\'s documents use', () => {
        const html = '<details><summary>More</summary><p>a<br>b <kbd>Ctrl</kbd></p></details>' +
            '<table><tr><td><span>x</span></td></tr></table>' +
            '<mark class="search-match" data-original="true">m</mark>';
        const root = parse(sanitizeMarkdownHtml(html));
        expect(root.querySelector('details > summary')?.textContent).toBe('More');
        expect(root.querySelector('br')).not.toBeNull();
        expect(root.querySelector('kbd')?.textContent).toBe('Ctrl');
        expect(root.querySelector('td span')?.textContent).toBe('x');
        expect(root.querySelector('mark')?.getAttribute('data-original')).toBe('true');
    });

    it('keeps the code header button with its inline icon', () => {
        const html = '<button type="button" class="code-copy" aria-label="Copy code" title="Copy code">' +
            '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11"></rect></svg><span>Copy</span></button>';
        const button = parse(sanitizeMarkdownHtml(html)).querySelector('button.code-copy');
        expect(button?.getAttribute('type')).toBe('button');
        expect(button?.getAttribute('aria-label')).toBe('Copy code');
        expect(button?.querySelector('svg rect')).not.toBeNull();
    });

    it.each([
        'javascript:alert(1)',
        'JaVaScRiPt:alert(1)',
        'java\tscript:alert(1)',
        'java&#x09;script:alert(1)',
        'vbscript:msgbox(1)',
        'data:text/html,<script>alert(1)</script>',
    ])('removes the href from an anchor whose value is %j', (href) => {
        const root = parse(sanitizeMarkdownHtml(`<a id="bad" href="${href}">x</a>`));
        expect(root.querySelector('#bad')).not.toBeNull();
        expect(root.querySelector('#bad')?.hasAttribute('href')).toBe(false);
    });

    it('keeps a data: image source on an img tag', () => {
        const root = parse(sanitizeMarkdownHtml('<img id="png" src="data:image/png;base64,AAAA">'));
        expect(root.querySelector('#png')?.getAttribute('src')).toBe('data:image/png;base64,AAAA');
    });
});

describe('sanitizeSvg', () => {
    it('keeps diagram structure and drops scripts, handlers and javascript: links', () => {
        const svg = '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">' +
            '<script>alert(1)</script>' +
            '<g id="node1" class="node" onclick="alert(2)"><path d="M0 0L1 1"></path><text>A</text></g>' +
            '<a xlink:href="javascript:alert(3)"><text>bad</text></a>' +
            '<a xlink:href="https://example.com"><text>good</text></a></svg>';
        const root = parse(sanitizeSvg(svg));
        expect(root.querySelector('svg')).not.toBeNull();
        expect(root.querySelector('g#node1 path')).not.toBeNull();
        expect(root.querySelector('script')).toBeNull();
        expect(root.querySelector('g#node1')?.hasAttribute('onclick')).toBe(false);
        expect(root.innerHTML).not.toContain('javascript:');
        expect(root.innerHTML).toContain('https://example.com');
    });

    it('removes style elements from the SVG', () => {
        const svg = '<svg xmlns="http://www.w3.org/2000/svg"><style>text{fill:red}</style><text>a</text></svg>';
        const root = parse(sanitizeSvg(svg));
        expect(root.querySelector('style')).toBeNull();
        expect(root.querySelector('text')?.textContent).toBe('a');
    });
});
