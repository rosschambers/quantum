export type WifiSecurity = 'open' | 'wpa' | 'wpa2' | 'wpa3' | 'other';
export type WifiBand = 'two_four' | 'five' | 'six' | 'unknown';
export type Ipv4Method = 'auto' | 'manual';

export interface WifiConnectionDetails {
    ip_address: string | null;
    gateway: string | null;
    dns: string[];
    mac: string | null;
    frequency_mhz: number | null;
    ipv4_method: Ipv4Method;
    metered: boolean;
}

export interface ActiveWifi {
    ssid: string;
    signal_percent: number;
    security: WifiSecurity;
    details: WifiConnectionDetails | null;
}

export interface WifiNetwork {
    ssid: string;
    bssid: string;
    signal_percent: number;
    security: WifiSecurity;
    band: WifiBand;
    saved: boolean;
    active: boolean;
}

export interface SavedNetwork {
    id: string;
    ssid: string;
    security: WifiSecurity;
    autoconnect: boolean;
    in_range: boolean;
}

export interface WifiState {
    available: boolean;
    radio_enabled: boolean;
    scanning: boolean;
    active: ActiveWifi | null;
    networks: WifiNetwork[];
    saved: SavedNetwork[];
}
