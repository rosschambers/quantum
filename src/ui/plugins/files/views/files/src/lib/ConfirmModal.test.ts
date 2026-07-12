import { afterEach, expect, test, vi } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import ConfirmModal from './ConfirmModal.svelte';

// render() appends to document.body and does not auto-clean between tests, so
// each test explicitly tears down to avoid duplicate elements bleeding across
// tests, and all queries are scoped to this render's own container.
afterEach(cleanup);

function renderModal(): {
    onConfirm: ReturnType<typeof vi.fn>;
    onCancel: ReturnType<typeof vi.fn>;
    confirmButton: HTMLElement;
    cancelButton: HTMLElement;
    backdrop: HTMLElement;
} {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { container } = render(ConfirmModal, {
        props: {
            title: 'Delete permanently',
            message: 'This cannot be undone.',
            confirmLabel: 'Delete',
            onConfirm,
            onCancel,
        },
    });
    const confirmButton = container.querySelector('.btn.danger') as HTMLElement;
    const cancelButton = container.querySelector('.btn.ghost') as HTMLElement;
    const backdrop = container.querySelector('.backdrop') as HTMLElement;
    return { onConfirm, onCancel, confirmButton, cancelButton, backdrop };
}

test('clicking confirm calls onConfirm', async () => {
    const { onConfirm, onCancel, confirmButton } = renderModal();
    await fireEvent.click(confirmButton);
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();
});

test('clicking cancel calls onCancel', async () => {
    const { onConfirm, onCancel, cancelButton } = renderModal();
    await fireEvent.click(cancelButton);
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
});

test('pressing Enter confirms and Escape cancels', async () => {
    const { onConfirm, onCancel, backdrop } = renderModal();
    await fireEvent.keyDown(backdrop, { key: 'Enter' });
    expect(onConfirm).toHaveBeenCalledOnce();
    await fireEvent.keyDown(backdrop, { key: 'Escape' });
    expect(onCancel).toHaveBeenCalledOnce();
});

test('clicking the backdrop cancels', async () => {
    const { onCancel, backdrop } = renderModal();
    await fireEvent.click(backdrop);
    expect(onCancel).toHaveBeenCalledOnce();
});
