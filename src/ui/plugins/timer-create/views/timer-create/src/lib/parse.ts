/**
 * Parse a human duration string into seconds. Accepts any combination of
 * hour (`h`), minute (`m`), and second (`s`) components in any order, summed
 * together: `90s`, `45m`, `2h`, `1h30m`. Returns `null` for input that
 * contains no recognizable components, has leftover unmatched characters, or
 * sums to zero or a negative total.
 */
export function parseDurationToSecs(input: string): number | null {
    const trimmed = input.trim().toLowerCase();
    if (trimmed.length === 0) return null;

    const componentPattern = /(\d+)\s*([hms])/g;
    let total = 0;
    let consumed = 0;
    let match: RegExpExecArray | null;

    while ((match = componentPattern.exec(trimmed)) !== null) {
        const value = Number(match[1]);
        const unit = match[2];
        if (unit === 'h') total += value * 3600;
        else if (unit === 'm') total += value * 60;
        else total += value;
        consumed += match[0].length;
    }

    // Reject input that contained characters outside the matched components,
    // for example `banana` or `45x`.
    if (consumed !== trimmed.length) return null;
    if (total <= 0) return null;
    return total;
}

/**
 * Parse an `HH:MM` wall-clock time. Returns `null` when the input is malformed
 * or the hour is greater than 23 or the minute is greater than 59.
 */
export function parseTimeOfDay(input: string): { hour: number; minute: number } | null {
    const match = /^(\d{1,2}):(\d{2})$/.exec(input.trim());
    if (match === null) return null;
    const hour = Number(match[1]);
    const minute = Number(match[2]);
    if (hour > 23 || minute > 59) return null;
    return { hour, minute };
}
