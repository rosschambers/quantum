import { describe, test, expect } from 'vitest';
import {
    layoutRulerSegments,
    thumbGeometry,
    scrollTopForTrackPosition,
    hitTestMark,
    searchMarks,
    rowCenterFraction,
    MINIMUM_HEIGHT,
    type RulerMark,
} from './overviewRuler';

describe('layoutRulerSegments', () => {
    test('places a point mark at start times trackHeight with its kind minimum height', () => {
        const marks: RulerMark[] = [{ start: 0.5, extent: 0, kind: 'match' }];
        const segments = layoutRulerSegments(marks, 600);
        expect(segments).toEqual([{ top: 300, height: MINIMUM_HEIGHT.match, kind: 'match', lane: 'full', index: undefined }]);
    });

    test('keeps a mark at 1 inside the track', () => {
        const marks: RulerMark[] = [{ start: 1, extent: 0, kind: 'match' }];
        const [segment] = layoutRulerSegments(marks, 600);
        expect(segment.top).toBeLessThanOrEqual(600 - segment.height);
        expect(segment.top).toBeGreaterThanOrEqual(0);
    });

    test('drops non-finite marks and clamps out-of-range starts', () => {
        const marks: RulerMark[] = [
            { start: Number.NaN, extent: 0, kind: 'match' },
            { start: Number.POSITIVE_INFINITY, extent: 0, kind: 'match' },
            { start: -0.2, extent: 0, kind: 'added' },
            { start: 1.4, extent: 0, kind: 'removed' },
        ];
        const segments = layoutRulerSegments(marks, 600);
        expect(segments).toHaveLength(2);
        const added = segments.find((segment) => segment.kind === 'added');
        const removed = segments.find((segment) => segment.kind === 'removed');
        expect(added?.top).toBe(0);
        expect(removed?.top).toBeLessThanOrEqual(600 - (removed?.height ?? 0));
    });

    test('merges touching same-kind marks but keeps different kinds separate', () => {
        const touching: RulerMark[] = [
            { start: 0.1, extent: 0, kind: 'match' },
            { start: 0.1 + MINIMUM_HEIGHT.match / 600, extent: 0, kind: 'match' },
        ];
        const mergedSegments = layoutRulerSegments(touching, 600);
        expect(mergedSegments).toHaveLength(1);
        expect(mergedSegments[0].height).toBeGreaterThanOrEqual(MINIMUM_HEIGHT.match * 1.5);

        const separate: RulerMark[] = [
            { start: 0.1, extent: 0, kind: 'added' },
            { start: 0.1, extent: 0, kind: 'removed' },
        ];
        expect(layoutRulerSegments(separate, 600)).toHaveLength(2);
    });

    test('current-match marks never merge and are emitted last', () => {
        const marks: RulerMark[] = [
            { start: 0.5, extent: 0, kind: 'current-match', index: 0 },
            { start: 0.5, extent: 0, kind: 'current-match', index: 1 },
            { start: 0.9, extent: 0, kind: 'match', index: 2 },
        ];
        const segments = layoutRulerSegments(marks, 600);
        expect(segments).toHaveLength(3);
        expect(segments.at(-1)?.kind).toBe('current-match');
        expect(segments.at(-2)?.kind).toBe('current-match');
        expect(segments.filter((segment) => segment.kind === 'current-match')).toHaveLength(2);
    });

    test('extent mark height is max(extent times track, minimum)', () => {
        const small: RulerMark[] = [{ start: 0, extent: 0.001, kind: 'added' }];
        expect(layoutRulerSegments(small, 600)[0].height).toBe(MINIMUM_HEIGHT.added);

        const large: RulerMark[] = [{ start: 0, extent: 0.5, kind: 'added' }];
        expect(layoutRulerSegments(large, 600)[0].height).toBe(300);
    });

    test('lanes are full when only search marks exist, split when change marks are present', () => {
        const onlySearch: RulerMark[] = [{ start: 0.2, extent: 0, kind: 'match' }];
        expect(layoutRulerSegments(onlySearch, 600)[0].lane).toBe('full');

        const mixed: RulerMark[] = [
            { start: 0.2, extent: 0, kind: 'match' },
            { start: 0.4, extent: 0.01, kind: 'added' },
            { start: 0.6, extent: 0, kind: 'current-match' },
        ];
        const segments = layoutRulerSegments(mixed, 600);
        expect(segments.find((segment) => segment.kind === 'match')?.lane).toBe('search');
        expect(segments.find((segment) => segment.kind === 'added')?.lane).toBe('change');
        expect(segments.find((segment) => segment.kind === 'current-match')?.lane).toBe('full');
    });

    test('ten thousand random match marks on a 600px track produce at most 300 search-lane segments', () => {
        const marks: RulerMark[] = Array.from({ length: 10000 }, () => ({
            start: Math.random(),
            extent: 0,
            kind: 'match' as const,
        }));
        const segments = layoutRulerSegments(marks, 600);
        expect(segments.length).toBeLessThanOrEqual(300);
    });
});

describe('thumbGeometry', () => {
    test('is proportional to the scrolled fraction and visible fraction', () => {
        const geometry = thumbGeometry({ scrollTop: 300, clientHeight: 200, scrollHeight: 1200 }, 600);
        expect(geometry.top).toBeCloseTo(150, 5);
        expect(geometry.height).toBeCloseTo(100, 5);
    });

    test('fills the whole track when content fits without scrolling', () => {
        const geometry = thumbGeometry({ scrollTop: 0, clientHeight: 600, scrollHeight: 400 }, 600);
        expect(geometry.top).toBe(0);
        expect(geometry.height).toBe(600);
    });

    test('never shrinks the thumb below the 8px minimum', () => {
        const geometry = thumbGeometry({ scrollTop: 0, clientHeight: 10, scrollHeight: 100000 }, 600);
        expect(geometry.height).toBeGreaterThanOrEqual(8);
    });

    test('clamps so the thumb never runs past the end of the track', () => {
        const geometry = thumbGeometry({ scrollTop: 990, clientHeight: 200, scrollHeight: 1000 }, 600);
        expect(geometry.top + geometry.height).toBeLessThanOrEqual(600);
    });
});

describe('scrollTopForTrackPosition', () => {
    test('centers the clicked position in the viewport', () => {
        const view = { scrollTop: 0, clientHeight: 200, scrollHeight: 1200 };
        const result = scrollTopForTrackPosition(300, 600, view);
        expect(result).toBeCloseTo(1200 / 2 - 100, 5);
    });

    test('clamps at the top of the track', () => {
        const view = { scrollTop: 0, clientHeight: 200, scrollHeight: 1200 };
        expect(scrollTopForTrackPosition(0, 600, view)).toBe(0);
    });

    test('clamps at the bottom of the track', () => {
        const view = { scrollTop: 0, clientHeight: 200, scrollHeight: 1200 };
        expect(scrollTopForTrackPosition(600, 600, view)).toBe(1000);
    });
});

describe('hitTestMark', () => {
    const marks: RulerMark[] = [
        { start: 0.1, extent: 0, kind: 'match', index: 0 },
        { start: 0.8, extent: 0, kind: 'match', index: 1 },
    ];

    test('finds the nearest mark within tolerance', () => {
        const hit = hitTestMark(marks, 0.1 * 600 + 1, 600, 4);
        expect(hit?.index).toBe(0);
    });

    test('returns null outside tolerance', () => {
        expect(hitTestMark(marks, 300, 600, 4)).toBeNull();
    });
});

describe('searchMarks', () => {
    test('returns an empty array when positions length does not match count', () => {
        const positions = new Float64Array([0.1, 0.2]);
        expect(searchMarks(positions, 3, null)).toEqual([]);
    });

    test('flags the current match and keeps indexes, with zero extent', () => {
        const positions = new Float64Array([0.1, 0.2, 0.3]);
        const marks = searchMarks(positions, 3, 1);
        expect(marks).toEqual([
            { start: 0.1, extent: 0, kind: 'match', index: 0 },
            { start: 0.2, extent: 0, kind: 'current-match', index: 1 },
            { start: 0.3, extent: 0, kind: 'match', index: 2 },
        ]);
    });

    test('marks none as current when current is null', () => {
        const positions = new Float64Array([0.1]);
        const marks = searchMarks(positions, 1, null);
        expect(marks[0].kind).toBe('match');
    });
});

describe('rowCenterFraction', () => {
    test('computes the center fraction for padding 12/12 and row height 21', () => {
        const geometry = { paddingTop: 12, rowHeight: 21, paddingBottom: 12 };
        const fraction = rowCenterFraction(0, 10, geometry);
        expect(fraction).toBeCloseTo((12 + 0 * 21 + 21 / 2) / (12 + 10 * 21 + 12), 10);
    });

    test('computes the center fraction for padding 32/32', () => {
        const geometry = { paddingTop: 32, rowHeight: 21, paddingBottom: 32 };
        const fraction = rowCenterFraction(5, 20, geometry);
        expect(fraction).toBeCloseTo((32 + 5 * 21 + 21 / 2) / (32 + 20 * 21 + 32), 10);
    });
});
