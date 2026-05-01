/*!
andlock - Rust tool to count Android unlock patterns on n-dimensional nodes
Copyright (C) 2026  Juan Luis Leal Contreras (Kuenlun)

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use anyhow::Result;

pub fn pretty_compact_json<T: serde::Serialize>(value: &T) -> Result<String> {
    let v = serde_json::to_value(value)?;
    Ok(format_value(&v, 0))
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn numeric_array_formats_inline() {
        let nums = vec![1, 2, 3];
        let output = pretty_compact_json(&nums).unwrap();
        assert_eq!(output, "[1, 2, 3]");
    }

    #[test]
    fn object_array_formats_multiline() {
        let objects = vec![serde_json::json!({"a": 1}), serde_json::json!({"b": 2})];
        let output = pretty_compact_json(&objects).unwrap();
        assert!(
            output.contains('\n'),
            "expected multiline output but got: {output}"
        );
        assert!(
            output.contains("  \"a\""),
            "expected indented key but got: {output}"
        );
    }

    #[test]
    fn nested_object_indents_correctly() {
        let nested = serde_json::json!({"outer": {"inner": 42}});
        let output = pretty_compact_json(&nested).unwrap();
        assert!(
            output.contains("    \"inner\""),
            "expected 4-space indentation for nested key but got: {output}"
        );
    }

    #[test]
    fn output_is_valid_json_roundtrip() {
        let original = serde_json::json!({"nums": [1, 2], "obj": {"x": true}});
        let pretty = pretty_compact_json(&original).unwrap();
        let reparsed: serde_json::Value =
            serde_json::from_str(&pretty).expect("formatted output must be valid JSON");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn primitive_values_format_without_modification() {
        assert_eq!(pretty_compact_json(&"hello").unwrap(), "\"hello\"");
        assert_eq!(pretty_compact_json(&42).unwrap(), "42");
        assert_eq!(pretty_compact_json(&true).unwrap(), "true");
        assert_eq!(
            pretty_compact_json(&serde_json::Value::Null).unwrap(),
            "null"
        );
    }

    #[test]
    fn empty_containers_remain_compact() {
        let empty_array: Vec<i32> = vec![];
        assert_eq!(pretty_compact_json(&empty_array).unwrap(), "[]");

        let empty_object = serde_json::json!({});
        assert_eq!(pretty_compact_json(&empty_object).unwrap(), "{}");
    }

    #[test]
    fn serialization_error_propagates() {
        struct AlwaysFails;
        impl serde::Serialize for AlwaysFails {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("intentional failure"))
            }
        }
        assert!(pretty_compact_json(&AlwaysFails).is_err());
    }

    #[test]
    fn mixed_array_formats_multiline() {
        let mixed = vec![
            serde_json::json!({"a": 1}),
            serde_json::json!(42),
            serde_json::json!("string"),
        ];
        let output = pretty_compact_json(&mixed).unwrap();
        assert!(
            output.contains('\n'),
            "mixed array should be multiline but got: {output}"
        );
    }
}
