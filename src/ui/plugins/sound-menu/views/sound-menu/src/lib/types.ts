export interface AudioSink {
    name: string;
    description: string;
    volume_percent: number;
    muted: boolean;
}

export interface AudioDevice {
    index: number;
    name: string;
    description: string;
    volume_percent: number;
    muted: boolean;
    is_default: boolean;
    port: string | null;
}

export interface AudioStream {
    index: number;
    application_name: string;
    media_name: string;
    icon: string | null;
    volume_percent: number;
    muted: boolean;
    device_index: number;
}

export interface AudioCardProfile {
    name: string;
    description: string;
    available: boolean;
    sink_count: number;
    source_count: number;
}

export interface AudioCard {
    index: number;
    name: string;
    description: string;
    active_profile: string;
    profiles: AudioCardProfile[];
}

export interface AudioState {
    available: boolean;
    default_sink: AudioSink | null;
    default_source: AudioSink | null;
    sinks: AudioDevice[];
    sources: AudioDevice[];
    playback_streams: AudioStream[];
    recording_streams: AudioStream[];
    cards: AudioCard[];
}
