use async_trait::async_trait;
use std::sync::Arc;

use quantum_domain::{
    Action, ActionOutcome, ClipboardWriter, DomainError, Match, MatchScore, MenuAction, ProviderId,
    ProviderSource, Query,
};

/// Provider for inline arithmetic (and, in a later extension, unit
/// conversion). A query is treated as maths when it starts with `=`, contains
/// an arithmetic operator, or names a known function; the expression is
/// evaluated and the result offered as a copyable [`Match`].
pub struct CalcProvider {
    id: ProviderId,
    clipboard: Arc<dyn ClipboardWriter>,
}

impl CalcProvider {
    /// Create a new CalcProvider that copies results with `clipboard`.
    pub fn new(clipboard: Arc<dyn ClipboardWriter>) -> Self {
        Self {
            id: ProviderId::from("calc"),
            clipboard,
        }
    }
}

/// Decide whether `expression` looks like arithmetic worth evaluating.
/// `had_equals` is true when the original query carried a leading `=`, which
/// forces evaluation even for input that contains no operator or function.
fn looks_like_arithmetic(expression: &str, had_equals: bool) -> bool {
    if had_equals {
        return true;
    }
    if expression.contains(['+', '-', '*', '/', '%', '^']) {
        return true;
    }
    const FUNCTIONS: [&str; 6] = ["sqrt", "sin", "cos", "log", "abs", "round"];
    FUNCTIONS
        .iter()
        .any(|function| expression.contains(function))
}

/// Rewrite the calculator-friendly bare function names accepted in queries
/// (`sqrt`, `sin`, `cos`, `log`, `abs`) into evalexpr's builtin `math::`
/// namespace. `round` is left untouched because evalexpr already exposes it as
/// a bare builtin. Matching is longest-name-first and only rewrites a name
/// immediately followed by `(` so it does not corrupt unrelated substrings.
fn rewrite_functions(expression: &str) -> String {
    // Pairs of (query name, evalexpr name). `log` maps to base-10 logarithm,
    // the common calculator convention.
    const MAPPINGS: [(&str, &str); 5] = [
        ("sqrt", "math::sqrt"),
        ("sin", "math::sin"),
        ("cos", "math::cos"),
        ("log", "math::log10"),
        ("abs", "math::abs"),
    ];

    let bytes = expression.as_bytes();
    let mut output = String::with_capacity(expression.len());
    let mut characters = expression.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let matched = MAPPINGS.iter().copied().find(|(name, _)| {
            let end = index + name.len();
            end <= bytes.len()
                && &bytes[index..end] == name.as_bytes()
                && !is_identifier_byte_before(bytes, index)
                && is_call_open_after(bytes, end)
        });
        if let Some((name, replacement)) = matched {
            output.push_str(replacement);
            // Consume the remaining bytes of the matched (ASCII) name; the
            // first character was already taken from the iterator above.
            for _ in 1..name.len() {
                characters.next();
            }
        } else {
            output.push(character);
        }
    }
    output
}

/// True when the byte immediately before `index` is part of an identifier, so
/// a candidate function name here is actually the tail of a longer word.
fn is_identifier_byte_before(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    let previous = bytes[index - 1];
    previous.is_ascii_alphanumeric() || previous == b'_' || previous == b':'
}

/// True when, ignoring spaces, the next non-space byte at or after `index` is
/// an opening parenthesis, marking a function call.
fn is_call_open_after(bytes: &[u8], index: usize) -> bool {
    let mut cursor = index;
    while cursor < bytes.len() && bytes[cursor] == b' ' {
        cursor += 1;
    }
    cursor < bytes.len() && bytes[cursor] == b'('
}

/// Format an evaluated number for display: trim a trailing `.0` so a whole
/// number reads `4` rather than `4.0`, while a genuine fraction such as
/// `3.6576` is preserved. Rounds to at most `max_decimals` decimal places and
/// strips trailing zeros introduced by that rounding.
fn format_number(value: f64, max_decimals: usize) -> String {
    if !value.is_finite() {
        // An infinite or NaN result is not a useful calculator answer.
        return value.to_string();
    }
    let rounded = format!("{value:.*}", max_decimals);
    if rounded.contains('.') {
        let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        rounded
    }
}

/// Evaluate `expression` as arithmetic. Returns the formatted result, or
/// `None` when the expression does not parse or evaluate to a finite number.
fn evaluate_arithmetic(expression: &str) -> Option<String> {
    let rewritten = rewrite_functions(expression);
    match evalexpr::eval_number(&rewritten) {
        Ok(value) if value.is_finite() => Some(format_number(value, 6)),
        Ok(_) => None,
        Err(_) => None,
    }
}

impl CalcProvider {
    /// Build the single result [`Match`] for a computed `value` produced from
    /// the echoed `expression`. Offers copy-result and copy-expression menu
    /// actions in addition to the default copy-result action.
    fn build_match(&self, value: String, expression: String) -> Match {
        Match {
            id: "calc".to_string(),
            provider: self.id.clone(),
            title: value.clone(),
            subtitle: Some(expression.clone()),
            icon: None,
            score: MatchScore::new(1.0),
            action: Action::Copy {
                text: value.clone(),
            },
            actions: vec![
                MenuAction {
                    label: "Copy result".to_string(),
                    icon: None,
                    danger: false,
                    action: Action::Copy { text: value },
                },
                MenuAction {
                    label: "Copy expression".to_string(),
                    icon: None,
                    danger: false,
                    action: Action::Copy { text: expression },
                },
            ],
        }
    }
}

#[async_trait]
impl ProviderSource for CalcProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    async fn search(&self, q: &Query) -> Result<Vec<Match>, DomainError> {
        let raw = q.text.trim();
        let (expression, had_equals) = match raw.strip_prefix('=') {
            Some(rest) => (rest.trim(), true),
            None => (raw, false),
        };

        if expression.is_empty() {
            return Ok(vec![]);
        }

        if !looks_like_arithmetic(expression, had_equals) {
            return Ok(vec![]);
        }

        match evaluate_arithmetic(expression) {
            Some(value) => Ok(vec![self.build_match(value, expression.to_string())]),
            None => Ok(vec![]),
        }
    }

    async fn invoke(&self, action: &Action) -> Result<ActionOutcome, DomainError> {
        match action {
            Action::Copy { text } => {
                self.clipboard.write_text(text).await?;
                Ok(ActionOutcome {
                    message: Some("Copied".to_string()),
                })
            }
            _ => Err(DomainError::Unsupported(
                "only Copy action supported".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeClipboard {
        writes: Arc<tokio::sync::RwLock<Vec<String>>>,
    }

    impl FakeClipboard {
        fn new() -> Self {
            Self {
                writes: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl ClipboardWriter for FakeClipboard {
        async fn write_text(&self, text: &str) -> Result<(), DomainError> {
            self.writes.write().await.push(text.to_string());
            Ok(())
        }

        async fn write_bytes(&self, _mime: &str, _bytes: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn provider() -> CalcProvider {
        CalcProvider::new(Arc::new(FakeClipboard::new()))
    }

    #[tokio::test]
    async fn sqrt_with_equals_prefix_evaluates() {
        let matches = provider().search(&Query::new("= sqrt(256)")).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "16");
    }

    #[tokio::test]
    async fn plain_addition_without_prefix_evaluates() {
        let matches = provider().search(&Query::new("2+2")).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "4");
    }

    #[tokio::test]
    async fn multiplication_with_decimal_evaluates() {
        let matches = provider().search(&Query::new("= 1920*0.15")).await.unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "288");
    }

    #[tokio::test]
    async fn plain_word_returns_no_matches() {
        let matches = provider().search(&Query::new("firefox")).await.unwrap();
        assert_eq!(matches.len(), 0);
    }

    #[tokio::test]
    async fn equals_prefixed_nonsense_returns_no_matches() {
        let matches = provider()
            .search(&Query::new("= not an expression"))
            .await
            .unwrap();
        assert_eq!(matches.len(), 0);
    }

    #[tokio::test]
    async fn invoke_copy_writes_result_to_clipboard() {
        let clipboard = Arc::new(FakeClipboard::new());
        let provider = CalcProvider::new(clipboard.clone());
        let action = Action::Copy {
            text: "16".to_string(),
        };

        let outcome = provider.invoke(&action).await.unwrap();

        assert!(outcome.message.is_some());
        let writes = clipboard.writes.read().await;
        assert_eq!(writes.as_slice(), ["16".to_string()]);
    }

    #[tokio::test]
    async fn invoke_non_copy_action_fails() {
        let action = Action::Launch {
            desktop_id: "x".to_string(),
        };
        assert!(provider().invoke(&action).await.is_err());
    }

    #[test]
    fn format_number_trims_trailing_point_zero() {
        assert_eq!(format_number(4.0, 6), "4");
    }

    #[test]
    fn format_number_keeps_real_fraction() {
        assert_eq!(format_number(3.6576, 6), "3.6576");
    }
}
