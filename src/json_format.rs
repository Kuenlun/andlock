// SPDX-License-Identifier: MIT OR Apache-2.0
// andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
// Copyright (c) 2026 Juan Luis Leal Contreras (Kuenlun)

/// Formats `value` as JSON using the crate's "pretty-compact" rules:
/// numeric arrays render inline (`[1, 2, 3]`); every other container
/// expands across lines with two-space indentation; primitives use
/// `serde_json`'s normal display. The function is infallible — callers
/// own a [`serde_json::Value`] up front, typically built from primitive
/// fields whose `Into<Value>` impls cannot fail.
pub fn pretty_compact_json_value(value: &serde_json::Value) -> String {
    format_value(value, 0)
}

fn format_value(value: &serde_json::Value, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let next = "  ".repeat(indent + 1);
    match value {
        serde_json::Value::Array(arr)
            if !arr.is_empty() && arr.iter().all(serde_json::Value::is_number) =>
        {
            let items: Vec<String> = arr.iter().map(std::string::ToString::to_string).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return "[]".to_owned();
            }
            let items: Vec<String> = arr
                .iter()
                .map(|v| format!("{next}{}", format_value(v, indent + 1)))
                .collect();
            format!("[\n{}\n{pad}]", items.join(",\n"))
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_owned();
            }
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{next}\"{k}\": {}", format_value(v, indent + 1)))
                .collect();
            format!("{{\n{}\n{pad}}}", items.join(",\n"))
        }
        other => other.to_string(),
    }
}
