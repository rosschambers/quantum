use std::collections::HashMap;

pub fn tokens_to_css(tokens: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = tokens
        .keys()
        .filter(|k| !k.contains(';') && !k.contains('\n'))
        .collect();
    keys.sort();
    let mut s = String::from(":root {\n");
    for k in keys {
        let v = &tokens[k];
        s.push_str("  --");
        s.push_str(k);
        s.push_str(": ");
        s.push_str(v);
        s.push_str(";\n");
    }
    s.push_str("}\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_map_yields_empty_block() {
        let css = tokens_to_css(&HashMap::new());
        assert_eq!(css, ":root {\n}\n");
    }

    #[test]
    fn writes_token_as_css_var() {
        let mut t = HashMap::new();
        t.insert("color-bg".into(), "#0a0a0a".into());
        assert!(tokens_to_css(&t).contains("--color-bg: #0a0a0a;"));
    }

    #[test]
    fn output_is_deterministically_sorted() {
        let mut t = HashMap::new();
        t.insert("font-size".into(), "14px".into());
        t.insert("color-bg".into(), "#000".into());
        let css = tokens_to_css(&t);
        let bg_at = css.find("--color-bg").unwrap();
        let font_at = css.find("--font-size").unwrap();
        assert!(bg_at < font_at);
    }

    #[test]
    fn malformed_key_is_skipped() {
        let mut t = HashMap::new();
        t.insert("bad;key".into(), "value".into());
        assert_eq!(tokens_to_css(&t), ":root {\n}\n");
    }
}
