import { vi } from 'vitest';

// Vitest resolves the server entry by default; these components need the same
// dependency tracking and tick behavior as their client-side compiled effects.
vi.mock('svelte', async (importOriginal) => {
    const actual = await importOriginal<typeof import('svelte')>();
    const { tick, untrack } = await import('svelte/internal/client');
    return { ...actual, tick, untrack };
});
