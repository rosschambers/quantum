import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import StatusBar from './StatusBar.svelte';

function renderStatus(extra: Partial<Record<string, unknown>> = {}) {
    return render(StatusBar, {
        props: {
            itemCount: 42,
            selectionCount: 0,
            selectionBytes: 0,
            driveLabel: 'Home',
            driveFree: '82.3 GB',
            ...extra,
        },
    });
}

describe('StatusBar', () => {
    it('renders the item count and the drive free space', () => {
        const { getByText } = renderStatus();
        expect(getByText('42 items')).not.toBeNull();
        expect(getByText('Home: 82.3 GB free')).not.toBeNull();
    });

    it('hides the selection summary when nothing is selected', () => {
        const { container } = renderStatus({ selectionCount: 0 });
        expect(container.querySelector('.sel')?.textContent ?? '').toBe('');
    });

    it('shows a selection summary with a formatted size when items are selected', () => {
        const { getByText } = renderStatus({ selectionCount: 3, selectionBytes: 1536 });
        expect(getByText('3 selected (1.5 KB)')).not.toBeNull();
    });
});
