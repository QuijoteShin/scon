// scon/src/encoder.rs
// SCON Encoder — Value → SCON string

use crate::value::Value;
use indexmap::IndexMap;

pub struct Encoder {
    indent: usize,
    delimiter: char,
}

impl Encoder {
    pub fn new() -> Self {
        Self { indent: 1, delimiter: ',' }
    }

    pub fn with_indent(mut self, indent: usize) -> Self {
        self.indent = indent.max(1);
        self
    }

    pub fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    pub fn encode(&self, data: &Value) -> String {
        let mut buf = String::with_capacity(1024);
        match data {
            Value::Object(obj) if obj.is_empty() => buf.push_str("{}"),
            Value::Array(arr) if arr.is_empty() => buf.push_str("[]"),
            _ => self.encode_value(data, 0, &mut buf),
        }
        buf
    }

    fn encode_value(&self, value: &Value, depth: usize, buf: &mut String) {
        match value {
            Value::Object(obj) => self.encode_object(obj, depth, buf),
            Value::Array(arr) => self.encode_array_value(None, arr, depth, buf),
            _ => {
                self.write_primitive(value, buf);
            }
        }
    }

    fn encode_object(&self, obj: &IndexMap<String, Value>, depth: usize, buf: &mut String) {
        let mut first = true;
        for (key, val) in obj {
            if !first { buf.push('\n'); }
            first = false;

            match val {
                // Empty object
                Value::Object(inner) if inner.is_empty() => {
                    self.write_indent(depth, buf);
                    self.write_key(key, buf);
                    buf.push_str(": {}");
                }
                // Empty array
                Value::Array(arr) if arr.is_empty() => {
                    self.write_indent(depth, buf);
                    self.write_key(key, buf);
                    buf.push_str(": []");
                }
                // Primitive value
                v if v.is_primitive() => {
                    self.write_indent(depth, buf);
                    self.write_key(key, buf);
                    buf.push_str(": ");
                    self.write_primitive(v, buf);
                }
                // Array
                Value::Array(arr) => {
                    self.encode_array_value(Some(key), arr, depth, buf);
                }
                // Nested object
                Value::Object(inner) => {
                    self.write_indent(depth, buf);
                    self.write_key(key, buf);
                    buf.push(':');
                    buf.push('\n');
                    self.encode_object(inner, depth + 1, buf);
                }
                _ => {}
            }
        }
    }

    fn encode_array_value(&self, key: Option<&str>, arr: &[Value], depth: usize, buf: &mut String) {
        let len = arr.len();

        if len == 0 {
            self.write_indent(depth, buf);
            if let Some(k) = key {
                self.write_key(k, buf);
                buf.push_str(": []");
            } else {
                buf.push_str("[]");
            }
            return;
        }

        // Array of primitives → inline
        if arr.iter().all(|v| v.is_primitive()) {
            self.write_indent(depth, buf);
            if let Some(k) = key {
                self.write_key(k, buf);
            }
            buf.push('[');
            buf.push_str(&len.to_string());
            buf.push_str("]: ");
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    buf.push(self.delimiter);
                    buf.push(' ');
                }
                self.write_primitive(v, buf);
            }
            return;
        }

        // Array of objects with uniform keys → tabular
        if let Some(fields) = self.extract_tabular_fields(arr) {
            self.write_indent(depth, buf);
            if let Some(k) = key {
                self.write_key(k, buf);
            }
            buf.push('[');
            buf.push_str(&len.to_string());
            buf.push_str("]{");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 { buf.push(self.delimiter); }
                self.write_key(f, buf);
            }
            buf.push_str("}:");
            for item in arr {
                if let Value::Object(obj) = item {
                    buf.push('\n');
                    self.write_indent(depth + 1, buf);
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 {
                            buf.push(self.delimiter);
                            buf.push(' ');
                        }
                        if let Some(v) = obj.get(f) {
                            self.write_primitive(v, buf);
                        } else {
                            buf.push_str("null");
                        }
                    }
                }
            }
            return;
        }

        // Mixed / expanded array
        self.write_indent(depth, buf);
        if let Some(k) = key {
            self.write_key(k, buf);
        }
        buf.push('[');
        buf.push_str(&len.to_string());
        buf.push_str("]:");
        for item in arr {
            buf.push('\n');
            match item {
                v if v.is_primitive() => {
                    self.write_indent(depth + 1, buf);
                    buf.push_str("- ");
                    self.write_primitive(v, buf);
                }
                Value::Object(obj) if obj.is_empty() => {
                    self.write_indent(depth + 1, buf);
                    buf.push_str("- {}");
                }
                Value::Object(obj) => {
                    self.encode_object_as_list_item(obj, depth + 1, buf);
                }
                Value::Array(inner) if inner.is_empty() => {
                    self.write_indent(depth + 1, buf);
                    buf.push_str("- []");
                }
                Value::Array(inner) if inner.iter().all(|v| v.is_primitive()) => {
                    self.write_indent(depth + 1, buf);
                    buf.push_str("- [");
                    buf.push_str(&inner.len().to_string());
                    buf.push_str("]: ");
                    for (i, v) in inner.iter().enumerate() {
                        if i > 0 {
                            buf.push(self.delimiter);
                            buf.push(' ');
                        }
                        self.write_primitive(v, buf);
                    }
                }
                _ => {}
            }
        }
    }

    fn encode_object_as_list_item(&self, obj: &IndexMap<String, Value>, depth: usize, buf: &mut String) {
        if obj.is_empty() {
            self.write_indent(depth, buf);
            buf.push_str("- ");
            return;
        }

        let mut iter = obj.iter();
        let (first_key, first_val) = iter.next().unwrap();

        self.write_indent(depth, buf);
        buf.push_str("- ");
        self.write_key(first_key, buf);

        match first_val {
            v if v.is_primitive() => {
                buf.push_str(": ");
                self.write_primitive(v, buf);
            }
            Value::Array(arr) if arr.is_empty() => {
                buf.push_str(": []");
            }
            Value::Array(arr) if arr.iter().all(|v| v.is_primitive()) => {
                buf.push('[');
                buf.push_str(&arr.len().to_string());
                buf.push_str("]: ");
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        buf.push(self.delimiter);
                        buf.push(' ');
                    }
                    self.write_primitive(v, buf);
                }
            }
            Value::Object(inner) if inner.is_empty() => {
                buf.push_str(": {}");
            }
            Value::Object(inner) => {
                buf.push(':');
                buf.push('\n');
                self.encode_object(inner, depth + 2, buf);
            }
            _ => {
                buf.push(':');
            }
        }

        // Continuation fields
        for (key, val) in iter {
            buf.push('\n');
            match val {
                v if v.is_primitive() => {
                    self.write_indent(depth + 1, buf);
                    self.write_key(key, buf);
                    buf.push_str(": ");
                    self.write_primitive(v, buf);
                }
                Value::Array(arr) if arr.is_empty() => {
                    self.write_indent(depth + 1, buf);
                    self.write_key(key, buf);
                    buf.push_str(": []");
                }
                Value::Array(arr) => {
                    self.encode_array_value(Some(key), arr, depth + 1, buf);
                }
                Value::Object(inner) if inner.is_empty() => {
                    self.write_indent(depth + 1, buf);
                    self.write_key(key, buf);
                    buf.push_str(": {}");
                }
                Value::Object(inner) => {
                    self.write_indent(depth + 1, buf);
                    self.write_key(key, buf);
                    buf.push(':');
                    buf.push('\n');
                    self.encode_object(inner, depth + 2, buf);
                }
                _ => {}
            }
        }
    }

    fn extract_tabular_fields(&self, arr: &[Value]) -> Option<Vec<String>> {
        if arr.is_empty() { return None; }

        let first = match &arr[0] {
            Value::Object(obj) if !obj.is_empty() => obj,
            _ => return None,
        };

        let keys: Vec<String> = first.keys().cloned().collect();

        // All values must be primitive
        for v in first.values() {
            if !v.is_primitive() { return None; }
        }

        for item in &arr[1..] {
            match item {
                Value::Object(obj) => {
                    if obj.len() != keys.len() { return None; }
                    for k in &keys {
                        match obj.get(k) {
                            Some(v) if v.is_primitive() => {}
                            _ => return None,
                        }
                    }
                }
                _ => return None,
            }
        }

        Some(keys)
    }

    // --- Primitive writing ---

    fn write_primitive(&self, value: &Value, buf: &mut String) {
        match value {
            Value::Null => buf.push_str("null"),
            Value::Bool(true) => buf.push_str("true"),
            Value::Bool(false) => buf.push_str("false"),
            Value::Integer(n) => buf.push_str(&n.to_string()),
            Value::Float(n) => {
                let s = format!("{}", n);
                buf.push_str(&s);
            }
            Value::String(s) => self.write_string(s, buf),
            _ => {}
        }
    }

    fn write_string(&self, s: &str, buf: &mut String) {
        if self.is_safe_unquoted(s) {
            buf.push_str(s);
        } else {
            buf.push('"');
            self.escape_string(s, buf);
            buf.push('"');
        }
    }

    fn write_key(&self, key: &str, buf: &mut String) {
        if self.is_valid_unquoted_key(key) {
            buf.push_str(key);
        } else {
            buf.push('"');
            self.escape_string(key, buf);
            buf.push('"');
        }
    }

    fn escape_string(&self, s: &str, buf: &mut String) {
        for c in s.chars() {
            match c {
                '\\' => buf.push_str("\\\\"),
                '"' => buf.push_str("\\\""),
                '\n' => buf.push_str("\\n"),
                '\r' => buf.push_str("\\r"),
                '\t' => buf.push_str("\\t"),
                ';' => buf.push_str("\\;"),
                _ => buf.push(c),
            }
        }
    }

    fn is_safe_unquoted(&self, s: &str) -> bool {
        if s.is_empty() { return false; }
        if matches!(s, "true" | "false" | "null") { return false; }
        // Numeric check
        if s.parse::<i64>().is_ok() || s.parse::<f64>().is_ok() { return false; }
        // Delimiter check
        if s.contains(self.delimiter) { return false; }
        // Special chars
        for c in s.chars() {
            if matches!(c, ' ' | '\t' | ':' | '"' | '\\' | ';' | '@' | '#' | '{' | '[' | ']' | '}') {
                return false;
            }
        }
        true
    }

    fn is_valid_unquoted_key(&self, key: &str) -> bool {
        if key.is_empty() { return false; }
        if key.starts_with('#') { return false; }
        for c in key.chars() {
            if matches!(c, ':' | '[' | ']' | '{' | '}' | '"' | '\\' | ' ' | '\t' | ';' | '@' | '#' | ',') {
                return false;
            }
        }
        true
    }

    fn write_indent(&self, depth: usize, buf: &mut String) {
        let spaces = self.indent * depth;
        for _ in 0..spaces {
            buf.push(' ');
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}
