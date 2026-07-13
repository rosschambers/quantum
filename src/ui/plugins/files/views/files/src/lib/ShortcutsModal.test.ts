import { afterEach, expect, test, vi } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import ShortcutsModal from './ShortcutsModal.svelte';
import { SHORTCUT_GROUPS } from './shortcuts';

afterEach(cleanup);

test('renders a row for every hint', () => {
    const { container } = render(ShortcutsModal, { props: { onClose: vi.fn() } });
    const total = SHORTCUT_GROUPS.reduce((n, g) => n + g.hints.length, 0);
    expect(container.querySelectorAll('[data-hint-row]').length).toBe(total);
});
test('backdrop click closes', async () => {
    const onClose = vi.fn();
    const { container } = render(ShortcutsModal, { props: { onClose } });
    await fireEvent.click(container.querySelector('.backdrop') as HTMLElement);
    expect(onClose).toHaveBeenCalledOnce();
});
test('Escape closes', async () => {
    const onClose = vi.fn();
    const { container } = render(ShortcutsModal, { props: { onClose } });
    await fireEvent.keyDown(container.querySelector('.backdrop') as HTMLElement, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
});
