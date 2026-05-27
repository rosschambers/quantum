use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MatchScore(f32);

impl MatchScore {
    pub fn new(v: f32) -> Self {
        Self(v.clamp(0.0, 1.0))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_above_one() {
        assert_eq!(MatchScore::new(1.5).value(), 1.0);
    }

    #[test]
    fn clamps_below_zero() {
        assert_eq!(MatchScore::new(-0.5).value(), 0.0);
    }

    #[test]
    fn passes_through_in_range() {
        assert_eq!(MatchScore::new(0.42).value(), 0.42);
    }
}
