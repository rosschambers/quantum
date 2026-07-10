// The bridge between a menu action and the daemon: `runOperation` performs a
// `FileOperation` through the injected IPC, catches any failure, and returns a
// plain `{ ok, message }` result the App can turn into a toast. Keeping the
// try/catch and the human-readable summary here means the App's wiring (Task
// 24) stays a thin loop: build the operation, run it, toast the result. The
// menus themselves stay pure — they only hand a `FileOperation` to a callback.

import type { FileOperation } from '@quantum/client';

/** The single IPC method `runOperation` needs; kept minimal so tests inject a fake. */
export interface OperationIpc {
    operation(op: FileOperation): Promise<void>;
}

/** The outcome of running an operation: whether it succeeded and a message to toast. */
export interface OperationResult {
    ok: boolean;
    message: string;
}

/** Optional hooks around an operation run. */
export interface RunOperationHooks {
    /** Called once with the result, whether the operation succeeded or failed. */
    onDone?: (result: OperationResult) => void;
}

/** Pluralise a count of items for a status message without abbreviating. */
function itemCount(count: number): string {
    return count === 1 ? '1 item' : `${count} items`;
}

/**
 * A short, human-readable summary of an operation for a success toast. Mirrors
 * the operation kinds in `FileOperation`; the `never` fallthrough makes an
 * unhandled kind a compile error if the union grows.
 */
export function describeOperation(op: FileOperation): string {
    switch (op.kind) {
        case 'copy':
            return `Copied ${itemCount(op.sources.length)}`;
        case 'move':
            return `Moved ${itemCount(op.sources.length)}`;
        case 'rename':
            return `Renamed to ${op.new_name}`;
        case 'duplicate':
            return 'Duplicated';
        case 'new_folder':
            return `Created folder ${op.name}`;
        case 'new_file':
            return `Created file ${op.name}`;
        case 'trash':
            return `Moved ${itemCount(op.paths.length)} to trash`;
        case 'delete':
            return `Deleted ${itemCount(op.paths.length)} permanently`;
        case 'compress':
            return `Compressed ${itemCount(op.paths.length)}`;
        case 'extract':
            return 'Extracted archive';
        default: {
            const exhaustive: never = op;
            return exhaustive;
        }
    }
}

/**
 * Perform a file-system operation through the IPC, translating a rejection into
 * a failing result rather than a thrown error. On success the message is a
 * summary of what happened; on failure it is the error's message. `onDone` (if
 * given) fires with the same result in both cases.
 */
export async function runOperation(
    ipc: OperationIpc,
    op: FileOperation,
    hooks?: RunOperationHooks,
): Promise<OperationResult> {
    let result: OperationResult;
    try {
        await ipc.operation(op);
        result = { ok: true, message: describeOperation(op) };
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        result = { ok: false, message };
    }
    hooks?.onDone?.(result);
    return result;
}
