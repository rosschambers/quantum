import type { WifiSecurity, WifiBand } from './types';

/** Human-facing label for a security type, shown as text (no lock icon). */
export const SECURITY_LABEL: Record<WifiSecurity, string> = {
    open: 'Open',
    wpa: 'WPA',
    wpa2: 'WPA2',
    wpa3: 'WPA3',
    other: 'Secured',
};

/** Frequency-band label used in the band badge (gigahertz). */
export const BAND_LABEL: Record<WifiBand, string> = {
    two_four: '2.4',
    five: '5',
    six: '6',
    unknown: '?',
};
