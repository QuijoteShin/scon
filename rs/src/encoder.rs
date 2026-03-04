// scon/src/encoder.rs
// SCON Encoder — Value → SCON string

use crate::value::{Value, SconMap};

// Lookup tables — branch-free byte classification, vive en L1 cache (256 bytes c/u)
// UNSAFE_VALUE[b] = true si byte b requiere quoting en un valor SCON
const UNSAFE_VALUE: [bool; 256] = {
    let mut t = [false; 256];
    t[b' ' as usize] = true;
    t[b'\t' as usize] = true;
    t[b':' as usize] = true;
    t[b'"' as usize] = true;
    t[b'\\' as usize] = true;
    t[b';' as usize] = true;
    t[b'@' as usize] = true;
    t[b'#' as usize] = true;
    t[b'{' as usize] = true;
    t[b'[' as usize] = true;
    t[b']' as usize] = true;
    t[b'}' as usize] = true;
    t
};

// UNSAFE_KEY[b] = true si byte b requiere quoting en un key SCON
const UNSAFE_KEY: [bool; 256] = {
    let mut t = [false; 256];
    t[b':' as usize] = true;
    t[b'[' as usize] = true;
    t[b']' as usize] = true;
    t[b'{' as usize] = true;
    t[b'}' as usize] = true;
    t[b'"' as usize] = true;
    t[b'\\' as usize] = true;
    t[b' ' as usize] = true;
    t[b'\t' as usize] = true;
    t[b';' as usize] = true;
    t[b'@' as usize] = true;
    t[b'#' as usize] = true;
    t[b',' as usize] = true;
    t
};

// P2.3: Pre-computed spaces for write_indent
const INDENT_SPACES: &str = "                                                                ";

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
        self.encode_to(data, &mut buf);
        buf
    }

    // P2.4: Write to external buffer — avoids allocation per call
    pub fn encode_to(&self, data: &Value, buf: &mut String) {
        match data {
            Value::Object(obj) if obj.is_empty() => buf.push_str("{}"),
            Value::Array(arr) if arr.is_empty() => buf.push_str("[]"),
            _ => self.encode_value(data, 0, buf),
        }
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

    fn encode_object(&self, obj: &SconMap<compact_str::CompactString, Value>, depth: usize, buf: &mut String) {
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
            self.write_usize(len, buf);
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
            self.write_usize(len, buf);
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
                        if let Some(v) = obj.get(*f) {
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
        self.write_usize(len, buf);
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
                    self.write_usize(inner.len(), buf);
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

    fn encode_object_as_list_item(&self, obj: &SconMap<compact_str::CompactString, Value>, depth: usize, buf: &mut String) {
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
                self.write_usize(arr.len(), buf);
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

    // P9: Retorna &str refs a keys del IndexMap — evita clone de strings
    fn extract_tabular_fields<'a>(&self, arr: &'a [Value]) -> Option<Vec<&'a str>> {
        if arr.is_empty() { return None; }

        let first = match &arr[0] {
            Value::Object(obj) if !obj.is_empty() => obj,
            _ => return None,
        };

        let keys: Vec<&str> = first.keys().map(|k| k.as_str()).collect();

        for v in first.values() {
            if !v.is_primitive() { return None; }
        }

        for item in &arr[1..] {
            match item {
                Value::Object(obj) => {
                    if obj.len() != keys.len() { return None; }
                    for k in &keys {
                        match obj.get(*k) {
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

    // P2.1: Write usize without temporary String allocation
    #[inline]
    fn write_usize(&self, n: usize, buf: &mut String) {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(n));
    }

    fn write_primitive(&self, value: &Value, buf: &mut String) {
        match value {
            Value::Null => buf.push_str("null"),
            Value::Bool(true) => buf.push_str("true"),
            Value::Bool(false) => buf.push_str("false"),
            // P2.1: itoa — no temporary String for integers
            Value::Integer(n) => {
                let mut itoa_buf = itoa::Buffer::new();
                buf.push_str(itoa_buf.format(*n));
            }
            // P2.1: ryu — no temporary String for floats
            Value::Float(n) => {
                let mut ryu_buf = ryu::Buffer::new();
                buf.push_str(ryu_buf.format(*n));
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

    // P2.2: Escape by chunks — skip to next escapable char instead of char-by-char
    fn escape_string(&self, s: &str, buf: &mut String) {
        let bytes = s.as_bytes();
        let mut last_flush = 0;

        for (i, &b) in bytes.iter().enumerate() {
            let esc = match b {
                b'\\' => "\\\\",
                b'"'  => "\\\"",
                b'\n' => "\\n",
                b'\r' => "\\r",
                b'\t' => "\\t",
                b';'  => "\\;",
                _ => continue,
            };
            // Flush the clean segment before this escape
            if last_flush < i {
                buf.push_str(&s[last_flush..i]);
            }
            buf.push_str(esc);
            last_flush = i + 1;
        }
        // Flush remaining
        if last_flush < s.len() {
            buf.push_str(&s[last_flush..]);
        }
    }

    // Lookup table — branch-free character classification, L1 cache resident (256 bytes)
    // Bit 0 = unsafe for unquoted value, Bit 1 = unsafe for unquoted key
    fn is_safe_unquoted(&self, s: &str) -> bool {
        if s.is_empty() { return false; }
        if matches!(s, "true" | "false" | "null") { return false; }
        // P6: Byte check — evita parse::<i64>()/parse::<f64>() que allocan en error path
        let first = s.as_bytes()[0];
        if first.is_ascii_digit() || first == b'+' || first == b'-' || first == b'.' {
            return false;
        }
        let delim_byte = self.delimiter as u8;
        for &b in s.as_bytes() {
            if UNSAFE_VALUE[b as usize] || b == delim_byte {
                return false;
            }
        }
        true
    }

    fn is_valid_unquoted_key(&self, key: &str) -> bool {
        if key.is_empty() { return false; }
        if key.as_bytes()[0] == b'#' { return false; }
        for &b in key.as_bytes() {
            if UNSAFE_KEY[b as usize] {
                return false;
            }
        }
        true
    }

    // P2.3: Use pre-computed slice instead of push loop
    fn write_indent(&self, depth: usize, buf: &mut String) {
        let spaces = self.indent * depth;
        if spaces == 0 { return; }
        if spaces <= INDENT_SPACES.len() {
            buf.push_str(&INDENT_SPACES[..spaces]);
        } else {
            // Fallback for very deep nesting
            let full = spaces / INDENT_SPACES.len();
            let rem = spaces % INDENT_SPACES.len();
            for _ in 0..full {
                buf.push_str(INDENT_SPACES);
            }
            buf.push_str(&INDENT_SPACES[..rem]);
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}
