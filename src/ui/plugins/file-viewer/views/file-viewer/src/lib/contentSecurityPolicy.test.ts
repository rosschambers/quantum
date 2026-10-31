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
        expect(policyIndex).toBeLessThan(firstActive);
    });

    it('never allows inline or evaluated script', () => {
        const scripts = directive(policy, 'script-src');
        expect(scripts.length).toBeGreaterThan(0);
        expect(scripts).not.toContain("'unsafe-inline'");
        expect(scripts).not.toContain("'unsafe-eval'");
    });

    it('shuts plugins, base rewriting, form posts and frames', () => {
        for (const name of ['object-src', 'base-uri', 'form-action', 'frame-src']) {
            expect(directive(policy, name)).toEqual(["'none'"]);
        }
    });
});
