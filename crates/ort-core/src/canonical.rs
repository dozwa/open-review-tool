//! Kanonisches JSON nach RFC 8785 (JCS), soweit es hier gebraucht wird.
//!
//! Schlüssel werden nach UTF-16-Codeeinheiten sortiert, es gibt keine Leerzeichen, Zeichenketten
//! werden wie bei `JSON.stringify` maskiert. Gleitkommazahlen sind nicht erlaubt: Ihre kanonische
//! Darstellung ist aufwendig, und Ereignisse brauchen sie nicht. So lässt sich jeder Hash auch
//! außerhalb der App nachrechnen.

use serde_json::Value;

use crate::error::{invalid, Result};

pub fn to_canonical(value: &Value) -> Result<String> {
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

fn write(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                out.push_str(&n.to_string());
            } else {
                return Err(invalid("Gleitkommazahlen sind in Ereignissen nicht erlaubt."));
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s)?),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key)?);
                out.push(':');
                write(&map[*key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_keys_and_strips_whitespace() {
        let v = json!({"b": 1, "a": [true, null, "x"], "c": {"z": "ü\n", "y": -3}});
        assert_eq!(to_canonical(&v).unwrap(), r#"{"a":[true,null,"x"],"b":1,"c":{"y":-3,"z":"ü\n"}}"#);
    }

    #[test]
    fn sorts_by_utf16_code_units() {
        // U+FF61 liegt in UTF-16 vor U+1F600 (Surrogat 0xD83D), obwohl der Codepunkt kleiner ist.
        let v = json!({"\u{1F600}": 1, "\u{FF61}": 2});
        assert_eq!(to_canonical(&v).unwrap(), "{\"\u{1F600}\":1,\"\u{FF61}\":2}");
    }

    #[test]
    fn escapes_control_characters_lowercase() {
        assert_eq!(to_canonical(&json!("a\u{1f}b\"/")).unwrap(), r#""a\u001fb\"/""#);
    }

    #[test]
    fn rejects_floats() {
        assert!(to_canonical(&json!({"x": 0.5})).is_err());
    }
}
