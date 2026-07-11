import { describe, it, expect } from 'vitest';
import type { Client, FileOperation } from '@quantum/client';
import { createFilesIpc } from './ipc';

/**
 * A fake `@quantum/client` `Client` that records every `call` and
 * `subscribe` invocation so the wrappers can be asserted against the
 * locked method names and parameter shapes without a real transport.
 */
function makeFakeClient(): {
    client: Client;
    calls: Array<[string, unknown]>;
    subscriptions: Array<[string, (payload: unknown) => void]>;
    unsubscribeCalls: number;
} {
    const calls: Array<[string, unknown]> = [];
    const subscriptions: Array<[string, (payload: unknown) => void]> = [];
    let unsubscribeCalls = 0;
    const client: Client = {
        call(method: string, params: unknown): Promise<unknown> {
            calls.push([method, params]);
            return Promise.resolve(undefined);
        },
        subscribe(channel: string, callback: (payload: unknown) => void): () => void {
            subscriptions.push([channel, callback]);
            return () => {
                unsubscribeCalls += 1;
            };
        },
        close(): void {},
    };
    return {
        client,
        calls,
        subscriptions,
        get unsubscribeCalls(): number {
            return unsubscribeCalls;
        },
    };
}

describe('createFilesIpc', () => {
    it('list calls files.list with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.list('/x');
        expect(fake.calls[0]).toEqual(['files.list', { path: '/x' }]);
    });

    it('places calls files.places with an empty parameter object', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.places();
        expect(fake.calls[0]).toEqual(['files.places', {}]);
    });

    it('pin calls files.pin with label and path', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.pin('L', '/p');
        expect(fake.calls[0]).toEqual(['files.pin', { label: 'L', path: '/p' }]);
    });

    it('unpin calls files.unpin with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.unpin('/p');
        expect(fake.calls[0]).toEqual(['files.unpin', { path: '/p' }]);
    });

    it('operation passes the operation object through to files.operation', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        const operation: FileOperation = {
            kind: 'rename',
            path: '/a/old',
            new_name: 'new',
        };
        ipc.operation(operation);
        expect(fake.calls[0]).toEqual(['files.operation', operation]);
    });

    it('open calls files.open with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.open('/f');
        expect(fake.calls[0]).toEqual(['files.open', { path: '/f' }]);
    });

    it('openWith calls files.open_with with path and desktop_id', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.openWith('/f', 'org.example.App');
        expect(fake.calls[0]).toEqual([
            'files.open_with',
            { path: '/f', desktop_id: 'org.example.App' },
        ]);
    });

    it('applications calls files.applications with an empty parameter object', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.applications();
        expect(fake.calls[0]).toEqual(['files.applications', {}]);
    });

    it('openTerminal calls files.open_terminal with a directory', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.openTerminal('/d');
        expect(fake.calls[0]).toEqual(['files.open_terminal', { directory: '/d' }]);
    });

    it('preview calls files.preview with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.preview('/f');
        expect(fake.calls[0]).toEqual(['files.preview', { path: '/f' }]);
    });

    it('search calls files.search with root, query, and limit', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.search('/r', 'needle', 50);
        expect(fake.calls[0]).toEqual([
            'files.search',
            { root: '/r', query: 'needle', limit: 50 },
        ]);
    });

    it('watch calls files.watch with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.watch('/f');
        expect(fake.calls[0]).toEqual(['files.watch', { path: '/f' }]);
    });

    it('unwatch calls files.unwatch with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.unwatch('/f');
        expect(fake.calls[0]).toEqual(['files.unwatch', { path: '/f' }]);
    });

    it('sizes calls files.sizes with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.sizes('/f');
        expect(fake.calls[0]).toEqual(['files.sizes', { path: '/f' }]);
    });

    it('cancelSizes calls files.cancel_sizes with a path parameter', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.cancelSizes('/f');
        expect(fake.calls[0]).toEqual(['files.cancel_sizes', { path: '/f' }]);
    });

    it('close calls view.hide with the canonical files view name', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        ipc.close();
        expect(fake.calls[0]).toEqual(['view.hide', { name: 'plugin/files/files' }]);
    });

    it('subscribeFilesEvents registers on the files.event channel', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        const callback = (): void => {};
        ipc.subscribeFilesEvents(callback);
        expect(fake.subscriptions[0][0]).toBe('files.event');
        expect(fake.subscriptions[0][1]).toBe(callback);
    });

    it('subscribeFilesEvents returns the client unsubscribe handle', () => {
        const fake = makeFakeClient();
        const ipc = createFilesIpc(fake.client);
        const unsubscribe = ipc.subscribeFilesEvents(() => {});
        expect(typeof unsubscribe).toBe('function');
        unsubscribe();
        expect(fake.unsubscribeCalls).toBe(1);
    });
});
