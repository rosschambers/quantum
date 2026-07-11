// Hand-written TypeScript mirror of the system tray domain DTOs in
// `src/domain/src/system_tray.rs`. There is no TypeScript codegen; these
// interfaces track the Rust types field-for-field using serde's snake_case
// field names.

/**
 * A reference to an icon. Mirrors the Rust `IconRef` enum, which serializes
 * with `#[serde(tag = "kind", content = "data", rename_all = "snake_case")]`,
 * so it is encoded as `{ kind, data }` where `kind` names the reference style
 * and `data` carries the icon name, filesystem path, or data URI.
 */
export type IconRef = { kind: 'name' | 'path' | 'data_uri'; data: string };

/** A single node in a system tray item's menu. Mirrors `SystemTrayMenuNode`. */
export interface SystemTrayMenuNode {
  id: number;
  label: string;
  enabled: boolean;
  visible: boolean;
  separator: boolean;
  toggle_type: string | null;
  toggle_state: boolean | null;
  icon_name: string | null;
  children: SystemTrayMenuNode[];
}

/** A single system tray item exposed by a StatusNotifierItem. Mirrors `SystemTrayItem`. */
export interface SystemTrayItem {
  service: string;
  title: string;
  tooltip: string;
  status: string;
  icon: IconRef | null;
  item_is_menu: boolean;
  menu: SystemTrayMenuNode[];
}

/** The full system tray snapshot. Mirrors `SystemTrayState`. */
export interface SystemTrayState {
  items: SystemTrayItem[];
}
