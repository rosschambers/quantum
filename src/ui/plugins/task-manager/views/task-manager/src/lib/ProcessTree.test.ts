import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import { PROCESSES_KILL, closeContextMenu, type Client, type ProcessNode } from '@quantum/client';
import ProcessTree from './ProcessTree.svelte';

/** Build a `ProcessNode` with sensible defaults for the tree component. */
function node(partial: Partial<ProcessNode> & { pid: number; name: string }): ProcessNode {
    const cpu = partial.aggregate_cpu_percent ?? 1;
    const mem = partial.aggregate_mem_bytes ?? 100 * 1024 * 1024;
    return {
        pid: partial.pid,
        name: partial.name,
        cpu_percent: partial.cpu_percent ?? cpu,
        mem_bytes: partial.mem_bytes ?? mem,
        aggregate_cpu_percent: cpu,
        aggregate_mem_bytes: mem,
        window: partial.window,
        protected: partial.protected ?? false,
        children: partial.children ?? [],
    };
}

const MEM_TOTAL = 32 * 1024 * 1024 * 1024;

/** A stub `Client` whose `call` is a spy; the default resolves. */
function stubClient(call: ReturnType<typeof vi.fn> = vi.fn(() => Promise.resolve(null))): {
    client: Client;
    call: ReturnType<typeof vi.fn>;
} {
    const client: Client = {
        call: call as unknown as Client['call'],
        subscribe: vi.fn(() => vi.fn()) as unknown as Client['subscribe'],
        close: vi.fn(),
    };
    return { client, call };
}

/** The buttons of the currently-open context menu, by their visible label. */
function menuButtons(): HTMLButtonElement[] {
    return Array.from(
        document.querySelectorAll<HTMLButtonElement>('[data-quantum-context-menu] button'),
    );
}

// Menu buttons render an optional leading icon glyph before the label, so match
// on a substring rather than exact text.
function menuButton(label: string): HTMLButtonElement | undefined {
    return menuButtons().find((button) => button.textContent?.includes(label));
}

// The context menu runtime appends to document.body, outside the component
// root, so testing-library's cleanup does not remove it; close it explicitly.
afterEach(() => {
    closeContextMenu();
    cleanup();
});

/**
 * Force a scroll container's measured geometry in jsdom (which never lays out,
 * so `clientHeight` is 0 and `scrollTop` is inert) and fire a scroll event so the
 * component re-measures. This drives the virtualization window deterministically.
 */
function drive(wrap: Element, scrollTop: number, clientHeight: number): Promise<boolean> {
    Object.defineProperty(wrap, 'clientHeight', { configurable: true, value: clientHeight });
    Object.defineProperty(wrap, 'scrollTop', { configurable: true, value: scrollTop });
    return fireEvent.scroll(wrap);
}

describe('ProcessTree virtualization', () => {
    // A tall single-section tree: 30 app leaves plus the two section headers.
    function tallApps(count: number): ProcessNode[] {
        return Array.from({ length: count }, (_unused, index) =>
            node({ pid: 1000 + index, name: `proc-${index}` }),
        );
    }

    it('renders only the windowed rows plus spacers, and scrolling reveals later rows', async () => {
        const { client } = stubClient();
        const { container } = render(ProcessTree, {
            props: { apps: tallApps(30), background: [], memTotalBytes: MEM_TOTAL, client },
        });

        const wrap = container.querySelector('.treewrap')!;
        // A viewport that fits roughly three rows over a 30-row tree.
        await drive(wrap, 0, 78);

        // Far fewer than 30 rows are in the DOM; the top of the list is present
        // and a deep row is not yet mounted.
        const mountedAtTop = container.querySelectorAll('tr[data-pid]').length;
        expect(mountedAtTop).toBeLessThan(30);
        expect(container.querySelector('tr[data-pid="1000"]')).not.toBeNull();
        expect(container.querySelector('tr[data-pid="1029"]')).toBeNull();
        // A bottom spacer row stands in for the un-mounted remainder.
        expect(container.querySelector('tr.spacer')).not.toBeNull();

        // Scroll to the end: the deep row mounts and the top row unmounts.
        await drive(wrap, 750, 78);
        expect(container.querySelector('tr[data-pid="1029"]')).not.toBeNull();
        expect(container.querySelector('tr[data-pid="1000"]')).toBeNull();
    });

    it('keeps sort, expansion and kill working on the windowed rows', async () => {
        const { client, call } = stubClient();
        const { container } = render(ProcessTree, {
            props: {
                apps: [
                    node({
                        pid: 4001,
                        name: 'firefox',
                        aggregate_cpu_percent: 5,
                        children: [node({ pid: 4011, name: 'renderer', aggregate_cpu_percent: 2 })],
                    }),
                    node({ pid: 4002, name: 'kitty', aggregate_cpu_percent: 9 }),
                ],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        const wrap = container.querySelector('.treewrap')!;
        await drive(wrap, 0, 400);

        // Default CPU-descending sort puts kitty (9) above firefox (5).
        const order = () =>
            Array.from(container.querySelectorAll<HTMLElement>('tr[data-pid]')).map(
                (r) => r.dataset.pid,
            );
        expect(order()[0]).toBe('4002');

        // Toggle the CPU sort to ascending; firefox now leads.
        const cpuSort = Array.from(container.querySelectorAll<HTMLButtonElement>('.sorter')).find(
            (b) => b.textContent?.includes('CPU'),
        )!;
        await fireEvent.click(cpuSort);
        expect(order()[0]).toBe('4001');

        // firefox is seeded expanded, so its child renders inside the window; a
        // right-click kill on the child still reaches processes.kill.
        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4011"]')!);
        await fireEvent.click(menuButton('End')!);
        expect(call).toHaveBeenCalledWith(PROCESSES_KILL, { pid: 4011, signal: 'term' });
    });
});

describe('ProcessTree kill menu', () => {
    it('opens End and Force kill on right-click of a normal row', async () => {
        const { client } = stubClient();
        const { container } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        const row = container.querySelector('tr[data-pid="4001"]')!;
        await fireEvent.contextMenu(row);

        expect(menuButton('End')).toBeTruthy();
        expect(menuButton('Force kill')).toBeTruthy();
    });

    it('offers only the protected notice on right-click of a protected row', async () => {
        const { client } = stubClient();
        const { container } = render(ProcessTree, {
            props: {
                apps: [],
                background: [node({ pid: 42, name: 'quantumd', protected: true })],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        // Background is collapsed by default; open it so the row renders.
        const backgroundToggle = Array.from(
            container.querySelectorAll<HTMLButtonElement>('.section-toggle'),
        ).find((button) => button.textContent?.includes('Background'))!;
        await fireEvent.click(backgroundToggle);
        const row = container.querySelector('tr[data-pid="42"]')!;
        await fireEvent.contextMenu(row);

        expect(menuButton('quantumd is protected')).toBeTruthy();
        expect(menuButton('End')).toBeUndefined();
        expect(menuButton('Force kill')).toBeUndefined();
    });
});

describe('ProcessTree kill flow', () => {
    it('calls processes.kill with the term signal and fades the subtree on End', async () => {
        const { client, call } = stubClient();
        const { container } = render(ProcessTree, {
            props: {
                apps: [
                    node({
                        pid: 4001,
                        name: 'firefox',
                        children: [node({ pid: 4011, name: 'renderer' })],
                    }),
                ],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('End')!);

        expect(call).toHaveBeenCalledWith(PROCESSES_KILL, { pid: 4001, signal: 'term' });
        // The whole subtree is marked dying (the expanded child too).
        await vi.waitFor(() => {
            expect(container.querySelector('tr[data-pid="4001"]')!.classList.contains('dying')).toBe(
                true,
            );
        });
    });

    it('force kill sends the kill signal', async () => {
        const { client, call } = stubClient();
        const { container } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('Force kill')!);

        expect(call).toHaveBeenCalledWith(PROCESSES_KILL, { pid: 4001, signal: 'kill' });
    });

    it('keeps a dying row faded across a snapshot that still lists it', async () => {
        const { client } = stubClient();
        const { container, rerender } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('End')!);
        await vi.waitFor(() =>
            expect(container.querySelector('tr[data-pid="4001"]')!.classList.contains('dying')).toBe(
                true,
            ),
        );

        // A fresh snapshot (new node instance, same pid) still lists firefox, so
        // it stays faded rather than snapping back.
        await rerender({
            apps: [node({ pid: 4001, name: 'firefox' })],
            background: [],
            memTotalBytes: MEM_TOTAL,
            client,
        });
        expect(container.querySelector('tr[data-pid="4001"]')!.classList.contains('dying')).toBe(
            true,
        );
    });

    it('drops a dying row when the next snapshot no longer lists it', async () => {
        const { client } = stubClient();
        const { container, rerender } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' }), node({ pid: 4002, name: 'kitty' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('End')!);
        await vi.waitFor(() =>
            expect(container.querySelector('tr[data-pid="4001"]')).not.toBeNull(),
        );

        // firefox has exited: the new snapshot omits it. The row disappears and
        // the surviving sibling is not left faded.
        await rerender({
            apps: [node({ pid: 4002, name: 'kitty' })],
            background: [],
            memTotalBytes: MEM_TOTAL,
            client,
        });
        expect(container.querySelector('tr[data-pid="4001"]')).toBeNull();
        expect(container.querySelector('tr[data-pid="4002"]')!.classList.contains('dying')).toBe(
            false,
        );
    });
});

describe('ProcessTree kill errors', () => {
    it('shows a dismissible error strip when a kill rejects, and rolls back the fade', async () => {
        const call = vi.fn(() => Promise.reject({ code: -32000, message: 'No such process' }));
        const { client } = stubClient(call);
        const { container, getByText, queryByText, getByRole } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('End')!);

        await vi.waitFor(() => expect(getByText('No such process')).toBeTruthy());
        // The optimistic fade is rolled back because the process did not die.
        expect(container.querySelector('tr[data-pid="4001"]')!.classList.contains('dying')).toBe(
            false,
        );

        await fireEvent.click(getByRole('button', { name: 'Dismiss error' }));
        expect(queryByText('No such process')).toBeNull();
    });

    it('clears a prior error on the next successful kill', async () => {
        const call = vi
            .fn()
            .mockImplementationOnce(() => Promise.reject({ code: 1, message: 'boom' }))
            .mockImplementationOnce(() => Promise.resolve(null));
        const { client } = stubClient(call);
        const { container, getByText, queryByText } = render(ProcessTree, {
            props: {
                apps: [node({ pid: 4001, name: 'firefox' }), node({ pid: 4002, name: 'kitty' })],
                background: [],
                memTotalBytes: MEM_TOTAL,
                client,
            },
        });

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4001"]')!);
        await fireEvent.click(menuButton('End')!);
        await vi.waitFor(() => expect(getByText('boom')).toBeTruthy());

        await fireEvent.contextMenu(container.querySelector('tr[data-pid="4002"]')!);
        await fireEvent.click(menuButton('End')!);
        await vi.waitFor(() => expect(queryByText('boom')).toBeNull());
    });
});
