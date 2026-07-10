import { describe, it, expect } from 'vitest';
import { friendlyProfileName, profileCountLabel } from './profiles';

describe('friendlyProfileName', () => {
    it('maps A2DP profiles to high-quality-no-mic', () => {
        expect(friendlyProfileName('a2dp-sink', 'A2DP Sink')).toBe('High quality (no mic)');
        expect(friendlyProfileName('a2dp_sink_sbc', 'raw')).toBe('High quality (no mic)');
    });
    it('maps headset/HSP/HFP profiles to headset-mic-on', () => {
        expect(friendlyProfileName('headset-head-unit', 'raw')).toBe(
            'Headset (mic on, lower quality)',
        );
        expect(friendlyProfileName('handsfree_head_unit', 'raw')).toBe(
            'Headset (mic on, lower quality)',
        );
    });
    it('maps off to Disabled', () => {
        expect(friendlyProfileName('off', 'Off')).toBe('Disabled');
    });
    it('falls back to the pactl description otherwise', () => {
        expect(friendlyProfileName('HiFi', 'Play HiFi quality Music')).toBe(
            'Play HiFi quality Music',
        );
    });
});

describe('profileCountLabel', () => {
    it('flags a profile with no sinks as no sound', () => {
        expect(profileCountLabel(0, 0)).toBe('0 out = no sound');
    });
    it('shows out and in counts otherwise', () => {
        expect(profileCountLabel(2, 1)).toBe('2 out / 1 in');
    });
});
