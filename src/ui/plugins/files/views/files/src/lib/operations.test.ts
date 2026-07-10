import { describe, it, expect, vi } from 'vitest';
import type { FileOperation } from '@quantum/client';
import { runOperation, describeOperation } from './operations';

describe('runOperation', () => {
    it('calls ipc.operation and returns a successful result', async () => {
        const operation = vi.fn().mockResolvedValue(undefined);
        const op: FileOperation = { kind: 'duplicate', path: '/home/user/a.txt' };
        const result = await runOperation({ operation }, op);
        expect(operation).toHaveBeenCalledWith(op);
        expect(result.ok).toBe(true);
        expect(result.message.length).toBeGreaterThan(0);
    });

    it('invokes onDone with the successful result', async () => {
        const operation = vi.fn().mockResolvedValue(undefined);
        const onDone = vi.fn();
        const op: FileOperation = { kind: 'trash', paths: ['/home/user/a.txt'] };
        const result = await runOperation({ operation }, op, { onDone });
        expect(onDone).toHaveBeenCalledWith(result);
        expect(result.ok).toBe(true);
    });

    it('returns a failing result carrying the error message on rejection', async () => {
        const operation = vi.fn().mockRejectedValue(new Error('permission denied'));
        const onDone = vi.fn();
        const op: FileOperation = { kind: 'delete', paths: ['/etc/passwd'] };
        const result = await runOperation({ operation }, op, { onDone });
        expect(result.ok).toBe(false);
        expect(result.message).toBe('permission denied');
        expect(onDone).toHaveBeenCalledWith(result);
    });

    it('stringifies a non-Error rejection reason', async () => {
        const operation = vi.fn().mockRejectedValue('boom');
        const result = await runOperation({ operation }, { kind: 'duplicate', path: '/x' });
        expect(result.ok).toBe(false);
        expect(result.message).toBe('boom');
    });
});

describe('describeOperation', () => {
    it('summarises a trash of several items', () => {
        expect(describeOperation({ kind: 'trash', paths: ['/a', '/b', '/c'] })).toBe(
            'Moved 3 items to trash',
        );
    });

    it('summarises a trash of a single item', () => {
        expect(describeOperation({ kind: 'trash', paths: ['/a'] })).toBe('Moved 1 item to trash');
    });

    it('names a rename destination', () => {
        expect(describeOperation({ kind: 'rename', path: '/a', new_name: 'b' })).toBe('Renamed to b');
    });

    it('names a new folder', () => {
        expect(describeOperation({ kind: 'new_folder', parent: '/', name: 'photos' })).toBe(
            'Created folder photos',
        );
    });
});
