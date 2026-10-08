// Batched DOM measurement for the shared overview ruler. Markdown and
// non-virtual text search do not key their matches by a fixed row height
// (unlike code/json/virtual text), so their match positions are read from
// live layout instead — one rect read for the scroll root, then one per
// anchor, never interleaved with a write (a read after a write anywhere in
// between would force a synchronous layout for every anchor, reintroducing
// the quadratic cost this whole ruler effort removes).

function rectForAnchor(anchor: Element | Range): DOMRect {
    const rect = anchor.getBoundingClientRect();
    if (rect.width !== 0 || rect.height !== 0 || !(anchor instanceof Element)) {
        return rect;
    }
    // A zero-size anchor (an empty inline match wrapper, or a collapsed
    // range) carries no usable position of its own: walk up to the nearest
    // ancestor with real layout and use its rect instead.
    let ancestor = anchor.parentElement;
    while (ancestor) {
        const ancestorRect = ancestor.getBoundingClientRect();
        if (ancestorRect.width !== 0 || ancestorRect.height !== 0) {
            return ancestorRect;
        }
        ancestor = ancestor.parentElement;
    }
    return rect;
}

/**
 * Reads `root`'s bounding rect once, then each anchor's rect, returning the
 * fraction `(anchorTop - rootTop) / rootHeight` for every anchor in order.
 * Performs no DOM writes. Returns an empty array when `root` has zero
 * height (nothing to measure against, and dividing by zero would produce
 * `NaN` for every anchor).
 */
export function measureFractions(root: HTMLElement, anchors: ArrayLike<Element | Range>): Float64Array {
    const rootRect = root.getBoundingClientRect();
    if (rootRect.height === 0) {
        return new Float64Array(0);
    }
    const result = new Float64Array(anchors.length);
    for (let index = 0; index < anchors.length; index++) {
        const rect = rectForAnchor(anchors[index]);
        result[index] = (rect.top - rootRect.top) / rootRect.height;
    }
    return result;
}

/**
 * Coalesces any number of `schedule()` calls that land in the same tick into
 * at most one `callback` invocation on the next animation frame. Shared by
 * `observeLayout`'s resize path and the overview ruler's scroll path, so
 * both read layout at most once per frame regardless of how many raw events
 * fired. `cancel()` drops a still-pending frame without running `callback`.
 */
export function coalesceToAnimationFrame(callback: () => void): { schedule: () => void; cancel: () => void } {
    let pendingFrame: number | null = null;

    return {
        schedule(): void {
            if (pendingFrame !== null) {
                return;
            }
            pendingFrame = requestAnimationFrame(() => {
                pendingFrame = null;
                callback();
            });
        },
        cancel(): void {
            if (pendingFrame !== null) {
                cancelAnimationFrame(pendingFrame);
                pendingFrame = null;
            }
        },
    };
}

/**
 * Watches `root` for layout changes (content resize, window resize, font
 * load, ...) via a single `ResizeObserver`, coalescing any number of
 * callbacks that land in the same tick into at most one `onChange` per
 * animation frame (via `coalesceToAnimationFrame`). Returns a function that
 * disconnects the observer and cancels any frame still pending.
 */
export function observeLayout(root: HTMLElement, onChange: () => void): () => void {
    const frame = coalesceToAnimationFrame(onChange);
    const observer = new ResizeObserver(() => {
        frame.schedule();
    });
    observer.observe(root);

    return () => {
        observer.disconnect();
        frame.cancel();
    };
}
