import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const html = readFileSync(resolve(__dirname, '../../index.html'), 'utf8');

function directive(policy: string, name: string): string[] {
    const entry = policy.split(';').map((part) => part.trim()).find((part) => part.startsWith(`${name} `));
    return entry ? entry.split(/\s+/).slice(1) : [];
}

describe('file viewer Content Security Policy', () => {
    const document = new DOMParser().parseFromString(html, 'text/html');
    const metas = document.querySelectorAll('meta[http-equiv="Content-Security-Policy"]');
    const policy = metas[0]?.getAttribute('content') ?? '';

    it('declares exactly one policy, before any script or style', () => {
        expect(metas).toHaveLength(1);
        const headChildren = Array.from(document.head.children);
        const policyIndex = headChildren.indexOf(metas[0]);
        const firstActive = headChildren.findIndex((element) => element.tagName === 'SCRIPT' || element.tagName === 'STYLE');
        expect(firstActive).toBeGreaterThanOrEqual(0);
        expect(policyIndex).toBeLessThan(firstActive);
    });

    it('never allows inline or evaluated script', () => {
        const scripts = directive(policy, 'script-src');
        expect(scripts.length).toBeGreaterThan(0);
        expect(scripts).not.toContain("'unsafe-inline'");
        expect(scripts).not.toContain("'unsafe-eval'");
    });

    it('locks the default source to the page itself', () => {
        expect(directive(policy, 'default-src')).toEqual(["'self'"]);
    });

    it('allows WebAssembly compilation and no other script source beyond the page', () => {
        const scripts = directive(policy, 'script-src');
        expect(scripts).toContain("'wasm-unsafe-eval'");
        for (const source of ['data:', 'blob:', 'https:', 'http:', '*']) {
            expect(scripts).not.toContain(source);
        }
    });

    it('lets images and media load only from the app scheme and local files', () => {
        for (const name of ['img-src', 'media-src']) {
            expect(directive(policy, name)).toContain('quantum:');
            expect(directive(policy, name)).toContain('file:');
        }
    });

    it('keeps network requests to the page itself', () => {
        expect(directive(policy, 'connect-src')).toEqual(["'self'"]);
    });

    it('shuts plugins, base rewriting, form posts and frames', () => {
        for (const name of ['object-src', 'base-uri', 'form-action', 'frame-src']) {
            expect(directive(policy, name)).toEqual(["'none'"]);
        }
    });
});
