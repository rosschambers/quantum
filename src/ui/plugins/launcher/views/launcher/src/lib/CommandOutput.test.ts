import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte/svelte5';
import type { ShellCaptureResult } from '@quantum/client';
import CommandOutput from './CommandOutput.svelte';

function result(overrides: Partial<ShellCaptureResult> = {}): ShellCaptureResult {
  return {
    command: 'echo hi',
    stdout: '',
    stderr: '',
    exit_code: 0,
    timed_out: false,
    ...overrides,
  };
}

describe('CommandOutput.svelte', () => {
  it('renders the standard output', () => {
    render(CommandOutput, { result: result({ stdout: 'hello world' }), running: false });
    expect(screen.getByText('hello world')).toBeDefined();
  });

  it('renders standard error in its own block when present', () => {
    render(CommandOutput, {
      result: result({ stdout: 'out', stderr: 'boom', exit_code: 1 }),
      running: false,
    });
    const stderrBlock = document.querySelector('.command-output-stderr');
    expect(stderrBlock).not.toBeNull();
    expect(stderrBlock?.textContent).toContain('boom');
  });

  it('does not render a standard error block when stderr is empty', () => {
    render(CommandOutput, { result: result({ stdout: 'out' }), running: false });
    expect(document.querySelector('.command-output-stderr')).toBeNull();
  });

  it('shows (no output) when both stdout and stderr are empty', () => {
    render(CommandOutput, { result: result(), running: false });
    expect(screen.getByText('(no output)')).toBeDefined();
  });

  it('shows the running state when running and no result yet', () => {
    render(CommandOutput, { result: null, running: true });
    expect(screen.getByText(/running/i)).toBeDefined();
  });

  it('shows the exit code in the header for a completed command', () => {
    render(CommandOutput, { result: result({ stdout: 'x', exit_code: 2 }), running: false });
    expect(screen.getByText('exit 2')).toBeDefined();
  });

  it('shows a timed out marker in the header when the command timed out', () => {
    render(CommandOutput, {
      result: result({ timed_out: true, exit_code: -1 }),
      running: false,
    });
    expect(screen.getByText('timed out')).toBeDefined();
  });
});
