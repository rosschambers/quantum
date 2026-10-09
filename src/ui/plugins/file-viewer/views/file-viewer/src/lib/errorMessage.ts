/**
 * Human-readable text for a rejected IPC call or thrown value. The client
 * rejects with a plain `{ code, message }` object rather than an `Error`, so a
 * bare `String(error)` would print "[object Object]".
 */
export function errorMessage(candidate: unknown): string {
    if (candidate && typeof candidate === 'object' && 'message' in candidate) {
        const message = (candidate as { message: unknown }).message;
        if (typeof message === 'string' && message.trim() !== '') {
            return message;
        }
    }
    if (typeof candidate === 'string' && candidate.trim() !== '') {
        return candidate;
    }
    return 'Request failed';
}
