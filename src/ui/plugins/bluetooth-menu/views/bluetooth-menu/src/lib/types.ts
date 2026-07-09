export interface BluetoothDevice {
    address: string;
    name: string;
    battery_percent: number | null;
    paired: boolean;
    trusted: boolean;
    connected: boolean;
    icon: string | null;
    rssi: number | null;
}

export interface BluetoothState {
    available: boolean;
    powered: boolean;
    discovering: boolean;
    devices: BluetoothDevice[];
    adapter_path: string;
}

export type BluetoothPairingRequestKind =
    | 'confirm'
    | 'request_passkey'
    | 'request_pin'
    | 'display_passkey'
    | 'authorize_service';

export interface PairingRequest {
    event: 'pairing_request';
    request: BluetoothPairingRequestKind;
    address: string;
    device_path: string;
    passkey: number | null;
    service_uuid: string | null;
}

export interface PairingCancelled {
    event: 'pairing_cancelled';
    address: string;
}
