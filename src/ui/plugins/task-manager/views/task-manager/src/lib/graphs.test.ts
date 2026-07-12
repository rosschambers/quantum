import { describe, it, expect } from 'vitest';
import { pushSample, hotColdColor, smoothPath, MAX_SAMPLES } from './graphs';

describe('pushSample', () => {
    it('appends a sample to the series', () => {
        expect(pushSample([1, 2], 3)).toEqual([1, 2, 3]);
    });

    it('starts empty and fills without backfill', () => {
        let series: number[] = [];
        series = pushSample(series, 42);
        expect(series).toEqual([42]);
    });

    it('caps the rolling series at MAX_SAMPLES, dropping the oldest', () => {
        let series: number[] = [];
        for (let i = 0; i < MAX_SAMPLES + 50; i += 1) {
            series = pushSample(series, i);
        }
        expect(series.length).toBe(MAX_SAMPLES);
        // Oldest 50 fell off the left; the newest sample is the last pushed.
        expect(series[0]).toBe(50);
        expect(series[series.length - 1]).toBe(MAX_SAMPLES + 49);
    });
});

describe('hotColdColor', () => {
    it('maps 0 to a cool blue-ish colour (blue channel dominant)', () => {
        expect(hotColdColor(0)).toBe('rgb(80, 180, 250)');
    });

    it('maps 100 to a hot red-ish colour (red channel dominant)', () => {
        expect(hotColdColor(100)).toBe('rgb(255, 50, 50)');
    });

    it('clamps out-of-range values to the endpoints', () => {
        expect(hotColdColor(-20)).toBe(hotColdColor(0));
        expect(hotColdColor(180)).toBe(hotColdColor(100));
    });
});

describe('smoothPath', () => {
    it('returns an empty string for fewer than two samples', () => {
        expect(smoothPath([], 100, 50, 100)).toBe('');
        expect(smoothPath([5], 100, 50, 100)).toBe('');
    });

    it('starts with a move command and emits one cubic segment per gap', () => {
        const path = smoothPath([0, 50, 100], 100, 50, 100);
        expect(path.startsWith('M')).toBe(true);
        // Three samples => two gaps => two cubic bezier segments.
        const cubicSegments = path.match(/C/g) ?? [];
        expect(cubicSegments.length).toBe(2);
    });

    it('scales larger series to one cubic segment per gap', () => {
        const samples = [10, 20, 30, 40, 25];
        const path = smoothPath(samples, 320, 56, 40);
        const cubicSegments = path.match(/C/g) ?? [];
        expect(cubicSegments.length).toBe(samples.length - 1);
    });
});
