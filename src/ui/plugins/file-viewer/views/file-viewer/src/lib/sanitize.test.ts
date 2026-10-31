import { describe, it, expect } from 'vitest';
import { sanitizeMarkdownHtml, sanitizeSvg } from './sanitize';
import { CODE_COPY_RESERVED_CLASSES } from './codeCopy';

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

    it('drops buttons but keeps their text, so a decoy copy control renders as plain content', () => {
        const root = parse(sanitizeMarkdownHtml('<p>Visible</p><button type="button" class="keep-me" onclick="alert(1)">Copy</button>'));
        expect(root.querySelector('button')).toBeNull();
        expect(root.textContent).toContain('Copy');
        expect(root.querySelector('p')?.textContent).toBe('Visible');
    });

    it('removes reserved code-header classes from author markup, whatever element carries them', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<div class="code-block"><div class="code-header"><span class="code-language">bash</span></div>' +
            '<pre><code>echo DECOY</code></pre></div>' +
            '<span class="code-copy-status">x</span><i class="code-copy-icon code-copy-label"></i>',
        ));
        for (const reserved of CODE_COPY_RESERVED_CLASSES) {
            expect(root.querySelector(`.${reserved}`)).toBeNull();
        }
        expect(root.textContent).toContain('echo DECOY');
        expect(root.textContent).toContain('bash');
    });

    it('keeps an author element\'s other classes when it drops reserved ones, and drops an empty class attribute', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<p id="mixed" class="code-block highlight">a</p><p id="only" class="code-copy">b</p>',
        ));
        expect(root.querySelector('#mixed')?.className).toBe('highlight');
        expect(root.querySelector('#only')?.hasAttribute('class')).toBe(false);
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

    it('strips inline style attributes so content cannot place an overlay over a code block', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<div style="position:absolute;inset:0;background:red">x</div><span style="color:red">y</span>',
        ));
        expect(root.querySelector('[style]')).toBeNull();
        expect(root.textContent).toBe('xy');
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

    it('keeps reserved class names in SVG, since only markdown is stripped', () => {
        const root = parse(sanitizeSvg('<svg><g class="code-block"><text>a</text></g></svg>'));
        expect(root.querySelector('g')?.getAttribute('class')).toBe('code-block');
    });
});
