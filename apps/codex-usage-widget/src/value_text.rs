use std::fmt::Write as _;

use serde_json::Value;

pub fn display_value(value: &Value) -> String {
    value.as_str().map_or_else(|| repr(value), str::to_owned)
}

fn repr(value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let items = fields
                .iter()
                .map(|(name, value)| format!("{}: {}", quoted(name), repr(value)))
                .collect::<Vec<_>>();
            format!("{{{}}}", items.join(", "))
        }
        Value::Array(values) => {
            let items = values.iter().map(repr).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(text) => quoted(text),
        Value::Number(_) => value.to_string(),
    }
}

fn quoted(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut output = String::from(quote);
    for ch in text.chars() {
        match ch {
            ch if ch == quote || ch == '\\' => {
                output.push('\\');
                output.push(ch);
            }
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() && u32::from(ch) <= 0xff => {
                let _ = write!(output, "\\x{:02x}", u32::from(ch));
            }
            ch => output.push(ch),
        }
    }
    output.push(quote);
    output
}
