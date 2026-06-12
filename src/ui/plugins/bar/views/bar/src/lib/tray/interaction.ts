// Shared interaction primitives for tray indicators.
//
// `onScroll` and `onClick` attach DOM listeners and return teardown
// functions. The Svelte components call them inside `$effect` so cleanup
// happens automatically on unmount.
//
// Scroll events are debounced to one event per 50ms — high-resolution
// wheels can fire dozens of events per turn, which would otherwise
// flood the action.invoke IPC.

export type ScrollHandler = (delta: 1 | -1) => void;
export type ClickHandler = () => void;
export type MouseButton = 'left' | 'right';

const SCROLL_DEBOUNCE_MS = 50;

export function onScroll(node: HTMLElement, handler: ScrollHandler): () => void {
    let last_fired = 0;
    const listener = (event: WheelEvent) => {
        event.preventDefault();
        const now = Date.now();
        if (now - last_fired < SCROLL_DEBOUNCE_MS) {
            return;
        }
        last_fired = now;
        const delta = event.deltaY < 0 ? 1 : -1;
        handler(delta);
    };
    node.addEventListener('wheel', listener, { passive: false });
    return () => {
        node.removeEventListener('wheel', listener);
    };
}

export function onClick(
    node: HTMLElement,
    handler: ClickHandler,
    button: MouseButton = 'left',
): () => void {
    if (button === 'right') {
        const listener = (event: MouseEvent) => {
            event.preventDefault();
            handler();
        };
        node.addEventListener('contextmenu', listener);
        return () => {
            node.removeEventListener('contextmenu', listener);
        };
    }
    const listener = () => {
        handler();
    };
    node.addEventListener('click', listener);
    return () => {
        node.removeEventListener('click', listener);
    };
}
