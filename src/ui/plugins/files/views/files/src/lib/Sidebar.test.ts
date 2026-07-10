import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import type { DriveInfo, Pin } from '@quantum/client';
import Sidebar from './Sidebar.svelte';
import { driveUsedFraction, driveBarClass } from './drive';

const GIGABYTE = 1024 * 1024 * 1024;

describe('driveUsedFraction', () => {
    it('is zero when the drive is entirely free', () => {
        const drive: DriveInfo = {
            label: 'Empty',
            mount_point: '/mnt/empty',
            total_bytes: GIGABYTE,
            free_bytes: GIGABYTE,
        };
        expect(driveUsedFraction(drive)).toBe(0);
    });

    it('guards against a zero-size drive', () => {
        const drive: DriveInfo = {
            label: 'Zero',
            mount_point: '/mnt/zero',
            total_bytes: 0,
            free_bytes: 0,
        };
        expect(driveUsedFraction(drive)).toBe(0);
    });

    it('reports the used fraction from free and total', () => {
        const drive: DriveInfo = {
            label: 'Half',
            mount_point: '/mnt/half',
            total_bytes: GIGABYTE,
            free_bytes: GIGABYTE / 2,
        };
        expect(driveUsedFraction(drive)).toBeCloseTo(0.5, 5);
    });
});

describe('driveBarClass', () => {
    it('is normal below the warning threshold', () => {
        expect(driveBarClass(0)).toBe('normal');
        expect(driveBarClass(0.5)).toBe('normal');
        expect(driveBarClass(0.75)).toBe('normal');
    });

    it('warns above 75 percent used', () => {
        expect(driveBarClass(0.8)).toBe('warn');
        expect(driveBarClass(0.9)).toBe('warn');
    });

    it('is critical above 90 percent used', () => {
        expect(driveBarClass(0.95)).toBe('crit');
    });
});

const NOOP_IPC = { list: vi.fn(() => Promise.resolve([])) } as unknown as never;

function renderSidebar(overrides: {
    pins?: Pin[];
    drives?: DriveInfo[];
    activePath?: string;
    onNavigate?: (path: string) => void;
    onUnpin?: (path: string) => void;
    onNavigateOther?: (path: string) => void;
}) {
    return render(Sidebar, {
        props: {
            pins: overrides.pins ?? [],
            drives: overrides.drives ?? [],
            activePath: overrides.activePath ?? '/',
            ipc: NOOP_IPC,
            onNavigate: overrides.onNavigate ?? vi.fn(),
            onUnpin: overrides.onUnpin ?? vi.fn(),
            onNavigateOther: overrides.onNavigateOther ?? vi.fn(),
        },
    });
}

describe('Sidebar drives', () => {
    it('renders the free-of-total text and no percent label', () => {
        const drives: DriveInfo[] = [
            {
                label: 'System',
                mount_point: '/',
                total_bytes: GIGABYTE,
                free_bytes: GIGABYTE / 2,
            },
        ];
        const { container, getByText } = renderSidebar({ drives });

        expect(getByText('512 MB free of 1 GB')).toBeTruthy();
        expect(container.querySelector('.pct')).toBeNull();
    });
});

describe('Sidebar pins', () => {
    it('navigates to a pin path when the pin is clicked', async () => {
        const onNavigate = vi.fn();
        const pins: Pin[] = [{ label: 'Documents', path: '/home/user/Documents' }];
        const { container } = renderSidebar({ pins, onNavigate });

        const pin = container.querySelector('.side-item') as HTMLElement;
        await fireEvent.click(pin);
        expect(onNavigate).toHaveBeenCalledWith('/home/user/Documents');
    });
});
