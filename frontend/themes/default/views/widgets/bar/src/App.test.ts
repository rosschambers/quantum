import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: vi.fn().mockResolvedValue(undefined),
        subscribe: vi.fn(() => () => {}),
        close: vi.fn(),
    }),
    __esModule: true,
}));

import App from './App.svelte';

describe('Bar App', () => {
    it('renders all three regions', () => {
        const { container } = render(App);
        expect(container.querySelector('.bar')).not.toBeNull();
        // ActiveWindow, SystemMeters, MediaControls all have their own root containers
        expect(container.querySelector('.active-window')).not.toBeNull();
        expect(container.querySelector('.meters')).not.toBeNull();
        expect(container.querySelector('.media-controls')).not.toBeNull();
    });
});
