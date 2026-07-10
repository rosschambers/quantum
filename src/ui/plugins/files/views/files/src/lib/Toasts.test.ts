import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import Toasts from './Toasts.svelte';
import { toasts, pushToast } from './toasts.svelte';

describe('Toasts', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        toasts.splice(0, toasts.length);
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('renders each toast message', async () => {
        pushToast('Copied report.txt');
        const { findByText } = render(Toasts);
        expect(await findByText('Copied report.txt')).not.toBeNull();
    });

    it('marks an error toast with the error class', async () => {
        pushToast('Operation failed', 'error');
        const { container } = render(Toasts);
        await vi.waitFor(() => {
            expect(container.querySelector('.toast.error')).not.toBeNull();
        });
    });
});
