/**
 * Map a launcher query to the set of providers it should be pinned to.
 *
 * A pinned provider list narrows the search fan-out: an empty query fetches the
 * default (usage-ranked) apps, and the punctuation prefixes route to a single
 * provider so that mode's result is the sole, top answer. A query with no
 * recognised prefix returns an empty list, which the search flow reads as
 * "fan out to every provider".
 *
 * This is the sibling of `parseCommandQuery` (the `$` capture path): the two are
 * deliberately separate because `$` runs a command inline rather than searching
 * a provider, so it never reaches this dispatch.
 *
 * Mappings:
 * - empty / whitespace only -> `desktop-apps`
 * - `=` prefix, or bare math -> `calc`
 * - `:` prefix -> `emoji`
 * - `;` prefix -> `clipboard`
 * - `>` or `!` prefix -> `shell`
 * - anything else -> `[]` (fan out to all providers)
 *
 * "Bare math" lets a user type an expression without any prefix: a query that
 * starts with a digit, an opening parenthesis, or a minus sign AND contains at
 * least one arithmetic operator (`+ - * / % ^`). A bare number with no operator
 * (for example `42`) is not treated as math, so it still searches normally.
 */

const MATH_OPERATORS = ['+', '-', '*', '/', '%', '^'];

function isBareMath(trimmed: string): boolean {
  const first = trimmed[0];
  const startsLikeMath = /[0-9]/.test(first) || first === '(' || first === '-';
  if (!startsLikeMath) {
    return false;
  }
  return MATH_OPERATORS.some((operator) => trimmed.includes(operator));
}

export function providersForQuery(text: string): string[] {
  const trimmed = text.trim();
  if (!trimmed) {
    return ['desktop-apps'];
  }
  if (trimmed.startsWith('=') || isBareMath(trimmed)) {
    return ['calc'];
  }
  if (trimmed.startsWith(':')) {
    return ['emoji'];
  }
  if (trimmed.startsWith(';')) {
    return ['clipboard'];
  }
  if (trimmed.startsWith('>') || trimmed.startsWith('!')) {
    return ['shell'];
  }
  return [];
}
