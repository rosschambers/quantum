import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import Breadcrumb from './Breadcrumb.svelte';
import { breadcrumbSegments } from './path';

describe('breadcrumbSegments', () => {
    it('returns a single root segment for the root path', () => {
        expect(breadcrumbSegments('/')).toEqual([{ label: '/', target: '/' }]);
    });

    it('splits a one-level path into root plus one directory', () => {
        expect(breadcrumbSegments('/home')).toEqual([
            { label: '/', target: '/' },
            { label: 'home', target: '/home' },
        ]);
    });

    it('accumulates absolute targets for a deep path', () => {
        expect(breadcrumbSegments('/home/user/Documents')).toEqual([
            { label: '/', target: '/' },
            { label: 'home', target: '/home' },
            { label: 'user', target: '/home/user' },
            { label: 'Documents', target: '/home/user/Documents' },
        ]);
    });
});

describe('Breadcrumb segments', () => {
    it('navigates to the absolute path of a clicked segment', async () => {
        const onNavigate = vi.fn();
        const { container } = render(Breadcrumb, {
            props: { path: '/home/user', onNavigate },
        });
        const segments = container.querySelectorAll('.seg');
        // Root, home, user — the second segment is "home" targeting "/home".
        expect(segments.length).toBe(3);
        await fireEvent.click(segments[1]);
        expect(onNavigate).toHaveBeenCalledTimes(1);
        expect(onNavigate).toHaveBeenCalledWith('/home');
    });
});

describe('Breadcrumb editing', () => {
    it('shows a text input prefilled with the path when editing is set', async () => {
        const onNavigate = vi.fn();
        const { container, rerender } = render(Breadcrumb, {
            props: { path: '/home/user', onNavigate, editing: false },
        });
        expect(container.querySelector('input')).toBeNull();
        await rerender({ path: '/home/user', onNavigate, editing: true });
        const input = container.querySelector('input') as HTMLInputElement;
        expect(input).not.toBeNull();
        expect(input.value).toBe('/home/user');
    });

    it('navigates to the typed value on Enter', async () => {
        const onNavigate = vi.fn();
        const { container } = render(Breadcrumb, {
            props: { path: '/home/user', onNavigate, editing: true },
        });
        const input = container.querySelector('input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '/etc' } });
        await fireEvent.keyDown(input, { key: 'Enter' });
        expect(onNavigate).toHaveBeenCalledWith('/etc');
    });

    it('enters editing when the empty filler space is clicked', async () => {
        const onNavigate = vi.fn();
        const { container } = render(Breadcrumb, {
            props: { path: '/home/user', onNavigate, editing: false },
        });
        expect(container.querySelector('input')).toBeNull();
        const filler = container.querySelector('.filler') as HTMLButtonElement;
        expect(filler).not.toBeNull();
        await fireEvent.click(filler);
        const input = container.querySelector('input') as HTMLInputElement;
        expect(input).not.toBeNull();
        expect(input.value).toBe('/home/user');
        expect(onNavigate).not.toHaveBeenCalled();
    });

    it('exits editing on Escape without navigating', async () => {
        const onNavigate = vi.fn();
        const { container } = render(Breadcrumb, {
            props: { path: '/home/user', onNavigate, editing: true },
        });
        const input = container.querySelector('input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '/etc' } });
        await fireEvent.keyDown(input, { key: 'Escape' });
        expect(onNavigate).not.toHaveBeenCalled();
        expect(container.querySelector('input')).toBeNull();
    });
});
