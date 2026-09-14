// The instance-qualified view name of THIS file-viewer window.
//
// A multi_instance view is opened as `plugin/file-viewer/file-viewer#<id>`.
// The host injects that full name as `window.__quantum_view_name`, so a
// close/hide must target it (not the bare shared name, which would resolve to
// a different window key and never close this instance). Falls back to the
// bare canonical name if the host predates the injection.
export function selfViewName(): string {
    const injected =
        typeof window !== 'undefined'
            ? (window as { __quantum_view_name?: string }).__quantum_view_name
            : undefined;
    return injected || 'plugin/file-viewer/file-viewer';
}
