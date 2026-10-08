// Pure math for the shared overview ruler: a 10px strip beside a scroll pane
// showing search matches (every renderer) and diff change marks (diff mode),
// a proportional thumb, and click-to-jump / click-to-select-mark hit testing.
// No DOM access here — see OverviewRuler.svelte for the component that reads
// layout and wires events.

export type RulerMarkKind = 'match' | 'current-match' | 'added' | 'removed' | 'mixed';

export interface RulerMark {
    /** Fractional position along the track, in [0, 1]. */
    start: number;
    /** Fractional extent along the track (0 for a point mark). */
    extent: number;
    kind: RulerMarkKind;
    index?: number;
}

export interface RulerSegment {
    top: number;
    height: number;
    kind: RulerMarkKind;
    lane: 'full' | 'change' | 'search';
    index?: number;
}

export interface ScrollView {
    scrollTop: number;
    clientHeight: number;
    scrollHeight: number;
}

export const MINIMUM_HEIGHT: Record<RulerMarkKind, number> = {
    match: 2,
    'current-match': 4,
    added: 2,
    removed: 2,
    mixed: 2,
};

function isChangeKind(kind: RulerMarkKind): boolean {
    return kind === 'added' || kind === 'removed' || kind === 'mixed';
}

function laneForKind(kind: RulerMarkKind, hasChangeKinds: boolean): 'full' | 'change' | 'search' {
    if (!hasChangeKinds) {
        return 'full';
    }
    if (isChangeKind(kind)) {
        return 'change';
    }
    if (kind === 'current-match') {
        return 'full';
    }
    return 'search';
}

interface PixelMark {
    top: number;
    height: number;
    kind: RulerMarkKind;
    lane: 'full' | 'change' | 'search';
    index?: number;
}

/**
 * Converts one mark to its pixel top/height within the track, or null when
 * the mark is dropped (a non-finite start). The start fraction is clamped to
 * [0, 1] first, then the resulting pixel top is clamped so the mark stays
 * fully inside the track (`top <= trackHeight - height`).
 */
function toPixelExtent(mark: RulerMark, trackHeight: number): { top: number; height: number } | null {
    if (!Number.isFinite(mark.start)) {
        return null;
    }
    const clampedStart = Math.min(1, Math.max(0, mark.start));
    const extent = Number.isFinite(mark.extent) ? mark.extent : 0;
    const height = Math.max(extent * trackHeight, MINIMUM_HEIGHT[mark.kind]);
    const rawTop = clampedStart * trackHeight;
    const top = Math.min(Math.max(rawTop, 0), Math.max(0, trackHeight - height));
    return { top, height };
}

export function layoutRulerSegments(marks: readonly RulerMark[], trackHeight: number): RulerSegment[] {
    const hasChangeKinds = marks.some((mark) => isChangeKind(mark.kind));

    const pixelMarks: PixelMark[] = [];
    for (const mark of marks) {
        const pixel = toPixelExtent(mark, trackHeight);
        if (!pixel) {
            continue;
        }
        pixelMarks.push({
            top: pixel.top,
            height: pixel.height,
            kind: mark.kind,
            lane: laneForKind(mark.kind, hasChangeKinds),
            index: mark.index,
        });
    }

    const currentMatches = pixelMarks.filter((mark) => mark.kind === 'current-match');
    const mergeable = pixelMarks.filter((mark) => mark.kind !== 'current-match');

    const groups = new Map<string, PixelMark[]>();
    for (const mark of mergeable) {
        const key = `${mark.kind}:${mark.lane}`;
        const group = groups.get(key);
        if (group) {
            group.push(mark);
        } else {
            groups.set(key, [mark]);
        }
    }

    const merged: RulerSegment[] = [];
    for (const group of groups.values()) {
        group.sort((a, b) => a.top - b.top);
        let current: RulerSegment | null = null;
        for (const mark of group) {
            if (current && mark.top <= current.top + current.height) {
                const bottom = Math.max(current.top + current.height, mark.top + mark.height);
                current.height = bottom - current.top;
                current.index = undefined;
            } else {
                current = { top: mark.top, height: mark.height, kind: mark.kind, lane: mark.lane, index: mark.index };
                merged.push(current);
            }
        }
    }

    merged.sort((a, b) => a.top - b.top);

    for (const mark of currentMatches) {
        merged.push({ top: mark.top, height: mark.height, kind: mark.kind, lane: mark.lane, index: mark.index });
    }

    return merged;
}

export function thumbGeometry(view: ScrollView, trackHeight: number): { top: number; height: number } {
    if (view.scrollHeight <= view.clientHeight || view.scrollHeight <= 0) {
        return { top: 0, height: trackHeight };
    }
    const rawHeight = Math.max(8, (view.clientHeight / view.scrollHeight) * trackHeight);
    const height = Math.min(rawHeight, trackHeight);
    const rawTop = (view.scrollTop / view.scrollHeight) * trackHeight;
    const top = Math.min(Math.max(rawTop, 0), trackHeight - height);
    return { top, height };
}

export function scrollTopForTrackPosition(y: number, trackHeight: number, view: ScrollView): number {
    const target = (y / trackHeight) * view.scrollHeight - view.clientHeight / 2;
    const maxScrollTop = Math.max(0, view.scrollHeight - view.clientHeight);
    return Math.min(Math.max(target, 0), maxScrollTop);
}

export function hitTestMark(
    marks: readonly RulerMark[],
    y: number,
    trackHeight: number,
    tolerance: number,
): RulerMark | null {
    let nearest: RulerMark | null = null;
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (const mark of marks) {
        const pixel = toPixelExtent(mark, trackHeight);
        if (!pixel) {
            continue;
        }
        const center = pixel.top + pixel.height / 2;
        const distance = Math.abs(center - y);
        if (distance < nearestDistance) {
            nearestDistance = distance;
            nearest = mark;
        }
    }
    if (nearest && nearestDistance <= tolerance) {
        return nearest;
    }
    return null;
}

export function searchMarks(positions: Float64Array, count: number, current: number | null): RulerMark[] {
    if (positions.length !== count) {
        return [];
    }
    const marks: RulerMark[] = [];
    for (let index = 0; index < count; index++) {
        marks.push({
            start: positions[index],
            extent: 0,
            kind: index === current ? 'current-match' : 'match',
            index,
        });
    }
    return marks;
}

export function rowCenterFraction(
    row: number,
    rowCount: number,
    geometry: { paddingTop: number; rowHeight: number; paddingBottom: number },
): number {
    const { paddingTop, rowHeight, paddingBottom } = geometry;
    const center = paddingTop + row * rowHeight + rowHeight / 2;
    const total = paddingTop + rowCount * rowHeight + paddingBottom;
    return center / total;
}
