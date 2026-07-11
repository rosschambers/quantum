//! Pure parser for com.canonical.dbusmenu layout trees.
//!
//! `GetLayout` returns `(revision: u32, layout)` where `layout` is a
//! recursive structure `(id: i32, properties: a{sv}, children: av)`. The
//! root node has id `0` and is a container that is never rendered, so
//! [`parse_menu_layout`] returns the root's children. Every value read is
//! downcast defensively: a failed downcast yields the property default or,
//! for a whole node, skips that node while its siblings still parse.

use std::collections::HashMap;

use quantum_domain::SystemTrayMenuNode;
use zbus::zvariant::{Dict, OwnedValue, Value};

/// The concrete wire type of the `layout` out-argument of
/// com.canonical.dbusmenu `GetLayout`: `(ia{sv}av)`.
///
/// Only the root node arrives as a concrete structure; its children (`av`)
/// are variants, each wrapping another node structure. Deserializing the
/// reply as `(u32, OwnedValue)` is wrong — that demands signature `uv`
/// (revision plus a variant) while the bus carries `u(ia{sv}av)`, so every
/// call fails with a signature mismatch before any parsing happens.
pub type RawMenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// Parse the root layout structure of a `GetLayout` reply into menu nodes.
///
/// The root node (id `0`) is a container that is never rendered, so its
/// children are returned. Malformed children are skipped, matching
/// [`parse_menu_layout`].
pub fn parse_menu_layout_reply(layout: &RawMenuLayout) -> Vec<SystemTrayMenuNode> {
    let (_, _, children) = layout;
    children
        .iter()
        .filter_map(|child| parse_node(child))
        .collect()
}

/// Parse the layout structure returned as the second element of
/// com.canonical.dbusmenu `GetLayout` into a list of menu nodes.
///
/// The argument is the recursive `(id, properties, children)` structure of
/// the root container. The root itself is never rendered, so its children
/// are returned. An unexpected top-level shape yields an empty vector, and
/// any individual malformed child is skipped rather than failing the whole
/// parse.
pub fn parse_menu_layout(layout: &Value<'_>) -> Vec<SystemTrayMenuNode> {
    let structure = match peel(layout) {
        Value::Structure(structure) => structure,
        _ => return Vec::new(),
    };
    let fields = structure.fields();
    let children = match fields.get(2).map(peel) {
        Some(Value::Array(array)) => array,
        _ => return Vec::new(),
    };
    children.iter().filter_map(parse_node).collect()
}

/// Strip accelerator markers from a dbusmenu label.
///
/// A single underscore marks the following character as the accelerator
/// and is dropped; a doubled underscore is a literal underscore. A
/// trailing lone underscore is dropped.
pub fn strip_accelerator_markers(label: &str) -> String {
    let mut result = String::with_capacity(label.len());
    let mut characters = label.chars();
    while let Some(character) = characters.next() {
        if character == '_' {
            match characters.next() {
                Some('_') => result.push('_'),
                Some(following) => result.push(following),
                None => {}
            }
        } else {
            result.push(character);
        }
    }
    result
}

/// Parse a single dbusmenu node structure into a [`SystemTrayMenuNode`].
///
/// Returns `None` when the value is not a well-formed
/// `(id, properties, children)` structure, so the caller can skip it.
fn parse_node(value: &Value<'_>) -> Option<SystemTrayMenuNode> {
    let structure = match peel(value) {
        Value::Structure(structure) => structure,
        _ => return None,
    };
    let fields = structure.fields();
    let identifier = match fields.first().map(peel) {
        Some(Value::I32(identifier)) => *identifier,
        _ => return None,
    };
    let properties = match fields.get(1).map(peel) {
        Some(Value::Dict(properties)) => properties,
        _ => return None,
    };

    let raw_label = property_string(properties, "label").unwrap_or_default();
    let node_type = property_string(properties, "type").unwrap_or_else(|| "standard".to_string());
    let separator = node_type == "separator";

    let children = match fields.get(2).map(peel) {
        Some(Value::Array(array)) => array.iter().filter_map(parse_node).collect(),
        _ => Vec::new(),
    };

    Some(SystemTrayMenuNode {
        id: identifier,
        label: strip_accelerator_markers(&raw_label),
        enabled: property_bool(properties, "enabled").unwrap_or(true),
        visible: property_bool(properties, "visible").unwrap_or(true),
        separator,
        toggle_type: property_string(properties, "toggle-type"),
        toggle_state: match property_i32(properties, "toggle-state") {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        icon_name: property_string(properties, "icon-name"),
        children,
    })
}

/// Peel any nested `Value::Value` variant wrappers, returning the inner
/// concrete value.
///
/// Over the wire a dbusmenu `av`/`sv` element arrives as a boxed variant
/// (`Value::Value`), whereas in-process constructed values may already be
/// the concrete variant. Peeling handles both shapes uniformly.
fn peel<'a, 'v>(value: &'a Value<'v>) -> &'a Value<'v> {
    let mut current = value;
    while let Value::Value(inner) = current {
        current = inner;
    }
    current
}

/// Look up a property by key in an `a{sv}` dictionary, peeling the value.
fn property<'a, 'v>(properties: &'a Dict<'v, 'v>, key: &str) -> Option<&'a Value<'v>> {
    for (property_key, property_value) in properties.iter() {
        if let Value::Str(candidate) = peel(property_key) {
            if candidate.as_str() == key {
                return Some(peel(property_value));
            }
        }
    }
    None
}

/// Read a string-typed property, or `None` when absent or the wrong type.
fn property_string(properties: &Dict<'_, '_>, key: &str) -> Option<String> {
    match property(properties, key) {
        Some(Value::Str(text)) => Some(text.as_str().to_string()),
        _ => None,
    }
}

/// Read a boolean-typed property, or `None` when absent or the wrong type.
fn property_bool(properties: &Dict<'_, '_>, key: &str) -> Option<bool> {
    match property(properties, key) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

/// Read an `i32`-typed property, or `None` when absent or the wrong type.
fn property_i32(properties: &Dict<'_, '_>, key: &str) -> Option<i32> {
    match property(properties, key) {
        Some(Value::I32(number)) => Some(*number),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    fn node<'a>(
        id: i32,
        properties: Vec<(&'static str, Value<'a>)>,
        children: Vec<Value<'a>>,
    ) -> Value<'a> {
        let mut map: HashMap<&str, Value<'a>> = HashMap::new();
        for (key, value) in properties {
            map.insert(key, value);
        }
        Value::from((id, map, children))
    }

    #[test]
    fn parses_labels_separators_and_defaults() {
        let layout = node(
            0,
            vec![("children-display", Value::from("submenu"))],
            vec![
                node(1, vec![("label", Value::from("_Library"))], vec![]),
                node(2, vec![("type", Value::from("separator"))], vec![]),
                node(
                    3,
                    vec![
                        ("label", Value::from("E_xit")),
                        ("enabled", Value::from(false)),
                    ],
                    vec![],
                ),
            ],
        );
        let parsed = parse_menu_layout(&layout);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].label, "Library");
        assert!(parsed[0].enabled && parsed[0].visible && !parsed[0].separator);
        assert!(parsed[1].separator);
        assert_eq!(parsed[2].label, "Exit");
        assert!(!parsed[2].enabled);
    }

    #[test]
    fn parses_nested_children_and_toggles() {
        let child = node(
            11,
            vec![
                ("label", Value::from("Enabled")),
                ("toggle-type", Value::from("checkmark")),
                ("toggle-state", Value::from(1i32)),
            ],
            vec![],
        );
        let layout = node(
            0,
            vec![],
            vec![node(
                10,
                vec![
                    ("label", Value::from("Settings")),
                    ("children-display", Value::from("submenu")),
                ],
                vec![child],
            )],
        );
        let parsed = parse_menu_layout(&layout);
        assert_eq!(parsed[0].children.len(), 1);
        assert_eq!(parsed[0].children[0].toggle_state, Some(true));
        assert_eq!(
            parsed[0].children[0].toggle_type.as_deref(),
            Some("checkmark")
        );
    }

    #[test]
    fn skips_malformed_children() {
        let layout = node(
            0,
            vec![],
            vec![
                Value::from("garbage"),
                node(1, vec![("label", Value::from("Ok"))], vec![]),
            ],
        );
        let parsed = parse_menu_layout(&layout);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].label, "Ok");
    }

    #[test]
    fn reply_type_signature_matches_dbusmenu_get_layout() {
        // Regression: the GetLayout reply body is `u(ia{sv}av)` on the wire.
        // Deserializing as `(u32, OwnedValue)` demanded `uv` and failed with
        // "Signature mismatch: got `u(ia{sv}av)`, expected `(uv)`" for every
        // application, leaving all tray menus permanently empty.
        use zbus::zvariant::Type;
        assert_eq!(
            <(u32, RawMenuLayout)>::signature().as_str(),
            "(u(ia{sv}av))"
        );
    }

    #[test]
    fn parses_reply_shaped_layout() {
        // Children arrive as variants (`av`) wrapping node structures, the
        // exact shape zbus produces when deserializing a GetLayout reply
        // into RawMenuLayout.
        let child_one = node(2, vec![("label", Value::from("Show Spotify"))], vec![]);
        let child_two = node(3, vec![("type", Value::from("separator"))], vec![]);
        let children = vec![
            OwnedValue::try_from(child_one).expect("owned child"),
            OwnedValue::try_from(child_two).expect("owned child"),
        ];
        let mut properties: HashMap<String, OwnedValue> = HashMap::new();
        properties.insert(
            "children-display".to_string(),
            OwnedValue::try_from(Value::from("submenu")).expect("owned property"),
        );
        let layout: RawMenuLayout = (0, properties, children);
        let parsed = parse_menu_layout_reply(&layout);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].label, "Show Spotify");
        assert!(parsed[1].separator);
    }

    #[test]
    fn strips_accelerator_markers() {
        assert_eq!(strip_accelerator_markers("_File"), "File");
        assert_eq!(strip_accelerator_markers("__literal"), "_literal");
        assert_eq!(strip_accelerator_markers("a_b_c"), "abc");
        assert_eq!(strip_accelerator_markers("plain"), "plain");
    }
}
