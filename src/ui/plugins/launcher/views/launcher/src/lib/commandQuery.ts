/**
 * The parsed intent of a launcher query with respect to command capture.
 * `capture` carries the shell command to run (the text after a leading `$`,
 * trimmed); `none` means the query is not a capture command and should follow
 * the normal results flow.
 */
export type CommandQuery = { mode: 'capture'; command: string } | { mode: 'none' };

/**
 * Parse a launcher query for the `$` capture prefix. A query that starts with
 * `$` and has a non-whitespace command after it becomes a `capture`; anything
 * else (a bare `$`, the `>`/`!` prefixes, a plain query, or empty) is `none`.
 */
export function parseCommandQuery(text: string): CommandQuery {
  if (!text.startsWith('$')) {
    return { mode: 'none' };
  }
  const command = text.slice(1).trim();
  if (command.length === 0) {
    return { mode: 'none' };
  }
  return { mode: 'capture', command };
}
