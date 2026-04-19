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
        serde_json::Value::Array(arr) if arr.iter().all(serde_json::Value::is_number) => {
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
