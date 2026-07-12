import { expect, test, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import ConfirmModal from './ConfirmModal.svelte';

test('clicking confirm calls onConfirm', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { getByText } = render(ConfirmModal, {
        props: {
            title: 'Delete permanently',
            message: 'This cannot be undone.',
            confirmLabel: 'Delete',
            onConfirm,
            onCancel,
        },
    });
    await fireEvent.click(getByText('Delete'));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();
});
