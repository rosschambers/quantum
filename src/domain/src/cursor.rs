use serde::{Deserialize, Serialize};

/// A pointer position in compositor-global (layout) coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_position_round_trips() {
        let position = CursorPosition { x: 12, y: -3 };
        let json = serde_json::to_string(&position).unwrap();
        let restored: CursorPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, position);
    }
}
