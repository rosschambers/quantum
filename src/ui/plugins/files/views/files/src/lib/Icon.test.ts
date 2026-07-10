import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import Icon, { type IconName } from './Icon.svelte';

const ALL_NAMES: IconName[] = [
    'folder',
    'file',
    'image',
    'code',
    'music',
    'archive',
    'document',
    'drive',
    'home',
    'pin',
    'back',
    'forward',
    'up',
    'chevron',
    'search',
    'columns',
    'link',
    'dotfile',
    'grid',
    'list',
    'eye',
];

describe('Icon', () => {
    it('renders an svg element for the folder icon', () => {
        const { container } = render(Icon, { props: { name: 'folder' } });
        const svg = container.querySelector('svg');
        expect(svg).not.toBeNull();
        expect(svg?.getAttribute('viewBox')).toBe('0 0 24 24');
    });

    it('renders an svg element for the drive icon', () => {
        const { container } = render(Icon, { props: { name: 'drive' } });
        expect(container.querySelector('svg')).not.toBeNull();
    });

    it('renders a non-empty svg for every named icon', () => {
        for (const name of ALL_NAMES) {
            const { container } = render(Icon, { props: { name } });
            const svg = container.querySelector('svg');
            expect(svg, `expected an svg for icon "${name}"`).not.toBeNull();
            expect(
                svg?.children.length ?? 0,
                `expected drawn content for icon "${name}"`,
            ).toBeGreaterThan(0);
        }
    });
});
