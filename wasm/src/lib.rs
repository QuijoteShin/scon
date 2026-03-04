// wasm/src/lib.rs
// SCON WebAssembly module — zero-intermediate encode/decode
//
// Decode: SCON string → TapeDecoder → tape → js_sys objects directly (no serde_json::Value)
// Encode: JsValue → walk JS objects → write SCON directly to String buffer (no Value intermediate)
//
// Same architecture as ext-php P19 (tape → Zval) and rs/ tape decoder.

use wasm_bindgen::prelude::*;
use js_sys::{Array, Object, Reflect};
use scon_core::{Minifier, TapeDecoder, Node};

// --- Decode: tape → JsValue directly ---

fn tape_to_js(nodes: &[Node], pos: &mut usize) -> JsValue {
    if *pos >= nodes.len() {
        return JsValue::NULL;
    }
    match &nodes[*pos] {
        Node::Null => { *pos += 1; JsValue::NULL }
        Node::Bool(b) => { let b = *b; *pos += 1; JsValue::from_bool(b) }
        Node::Integer(i) => { let i = *i; *pos += 1; JsValue::from_f64(i as f64) }
        Node::Float(f) => { let f = *f; *pos += 1; JsValue::from_f64(f) }
        Node::String(s) => { let s = *s; *pos += 1; JsValue::from_str(s) }
        Node::Array(count) => {
            let count = *count;
            *pos += 1;
            let arr = Array::new_with_length(count as u32);
            for i in 0..count {
                arr.set(i as u32, tape_to_js(nodes, pos));
            }
            arr.into()
        }
        Node::Object(count) => {
            let count = *count;
            *pos += 1;
            let obj = Object::new();
            for _ in 0..count {
                let key = if *pos < nodes.len() {
                    if let Node::Key(k) = &nodes[*pos] {
                        *pos += 1;
                        *k
                    } else {
                        *pos += 1;
                        ""
                    }
                } else {
                    break;
                };
                let val = tape_to_js(nodes, pos);
                let _ = Reflect::set(&obj, &JsValue::from_str(key), &val);
            }
            obj.into()
        }
        Node::Key(_) => { *pos += 1; JsValue::NULL }
    }
}

#[wasm_bindgen]
pub fn scon_decode(input: &str) -> Result<JsValue, JsError> {
    let mut decoder = TapeDecoder::new();
    let tape = decoder.decode(input)
        .map_err(|e| JsError::new(&e))?;
    let mut pos = 0;
    Ok(tape_to_js(&tape.nodes, &mut pos))
}

// --- Decode v2: tape → JSON string (single crossing, V8 JSON.parse materializes) ---

fn tape_to_json_string(nodes: &[Node], pos: &mut usize, buf: &mut String) {
    if *pos >= nodes.len() {
        buf.push_str("null");
        return;
    }
    match &nodes[*pos] {
        Node::Null => { *pos += 1; buf.push_str("null"); }
        Node::Bool(b) => {
            let b = *b;
            *pos += 1;
            buf.push_str(if b { "true" } else { "false" });
        }
        Node::Integer(i) => {
            let i = *i;
            *pos += 1;
            let mut b = itoa::Buffer::new();
            buf.push_str(b.format(i));
        }
        Node::Float(f) => {
            let f = *f;
            *pos += 1;
            let mut b = ryu::Buffer::new();
            buf.push_str(b.format(f));
        }
        Node::String(s) => {
            let s = *s;
            *pos += 1;
            buf.push('"');
            json_escape(s, buf);
            buf.push('"');
        }
        Node::Array(count) => {
            let count = *count;
            *pos += 1;
            buf.push('[');
            for i in 0..count {
                if i > 0 { buf.push(','); }
                tape_to_json_string(nodes, pos, buf);
            }
            buf.push(']');
        }
        Node::Object(count) => {
            let count = *count;
            *pos += 1;
            buf.push('{');
            for i in 0..count {
                if i > 0 { buf.push(','); }
                let key = if *pos < nodes.len() {
                    if let Node::Key(k) = &nodes[*pos] {
                        *pos += 1;
                        *k
                    } else {
                        *pos += 1;
                        ""
                    }
                } else {
                    break;
                };
                buf.push('"');
                json_escape(key, buf);
                buf.push_str("\":");
                tape_to_json_string(nodes, pos, buf);
            }
            buf.push('}');
        }
        Node::Key(_) => { *pos += 1; buf.push_str("null"); }
    }
}

// JSON string escaping (RFC 8259)
fn json_escape(s: &str, buf: &mut String) {
    let bytes = s.as_bytes();
    let mut last = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x08 => "\\b",
            0x0C => "\\f",
            _ if b < 0x20 => {
                if last < i { buf.push_str(&s[last..i]); }
                buf.push_str("\\u00");
                buf.push(char::from(b"0123456789abcdef"[(b >> 4) as usize]));
                buf.push(char::from(b"0123456789abcdef"[(b & 0xf) as usize]));
                last = i + 1;
                continue;
            }
            _ => continue,
        };
        if last < i { buf.push_str(&s[last..i]); }
        buf.push_str(esc);
        last = i + 1;
    }
    if last < s.len() { buf.push_str(&s[last..]); }
}

/// Decode SCON → JSON string (single WASM↔JS crossing, use with JSON.parse on JS side)
#[wasm_bindgen]
pub fn scon_to_json(input: &str) -> Result<String, JsError> {
    let mut decoder = TapeDecoder::new();
    let tape = decoder.decode(input)
        .map_err(|e| JsError::new(&e))?;
    let mut buf = String::with_capacity(input.len() * 2);
    let mut pos = 0;
    tape_to_json_string(&tape.nodes, &mut pos, &mut buf);
    Ok(buf)
}

// --- Encode: JsValue → SCON string directly ---

// Lookup tables — same as rs/encoder.rs
const UNSAFE_VALUE: [bool; 256] = {
    let mut t = [false; 256];
    t[b' ' as usize] = true;  t[b'\t' as usize] = true;
    t[b':' as usize] = true;  t[b'"' as usize] = true;
    t[b'\\' as usize] = true; t[b';' as usize] = true;
    t[b'@' as usize] = true;  t[b'#' as usize] = true;
    t[b'{' as usize] = true;  t[b'[' as usize] = true;
    t[b']' as usize] = true;  t[b'}' as usize] = true;
    t
};

const UNSAFE_KEY: [bool; 256] = {
    let mut t = [false; 256];
    t[b':' as usize] = true;  t[b'[' as usize] = true;
    t[b']' as usize] = true;  t[b'{' as usize] = true;
    t[b'}' as usize] = true;  t[b'"' as usize] = true;
    t[b'\\' as usize] = true; t[b' ' as usize] = true;
    t[b'\t' as usize] = true; t[b';' as usize] = true;
    t[b'@' as usize] = true;  t[b'#' as usize] = true;
    t[b',' as usize] = true;
    t
};

const INDENT_SPACES: &str = "                                                                ";

struct JsEncoder {
    indent: usize,
    buf: String,
}

impl JsEncoder {
    fn new(indent: usize) -> Self {
        Self { indent, buf: String::with_capacity(1024) }
    }

    fn encode(mut self, data: &JsValue) -> String {
        if data.is_null() || data.is_undefined() {
            return "null".into();
        }
        if let Some(b) = data.as_bool() {
            return if b { "true".into() } else { "false".into() };
        }
        if let Some(n) = data.as_f64() {
            return self.format_number(n);
        }
        if let Some(s) = data.as_string() {
            self.write_string_value(&s);
            return self.buf;
        }
        if Array::is_array(data) {
            let arr = Array::from(data);
            if arr.length() == 0 {
                return "[]".into();
            }
            self.encode_array_top(None, &arr, 0);
            return self.buf;
        }
        if data.is_object() {
            let obj = Object::from(data.clone());
            let keys = Object::keys(&obj);
            if keys.length() == 0 {
                return "{}".into();
            }
            self.encode_object(&obj, 0);
            return self.buf;
        }
        "null".into()
    }

    fn encode_object(&mut self, obj: &Object, depth: usize) {
        let keys = Object::keys(obj);
        let len = keys.length();
        for i in 0..len {
            if i > 0 { self.buf.push('\n'); }
            let key: String = keys.get(i).as_string().unwrap_or_default();
            let val = Reflect::get(obj, &keys.get(i)).unwrap_or(JsValue::NULL);

            if val.is_null() || val.is_undefined() {
                self.write_indent(depth);
                self.write_key(&key);
                self.buf.push_str(": null");
            } else if let Some(b) = val.as_bool() {
                self.write_indent(depth);
                self.write_key(&key);
                self.buf.push_str(if b { ": true" } else { ": false" });
            } else if let Some(n) = val.as_f64() {
                self.write_indent(depth);
                self.write_key(&key);
                self.buf.push_str(": ");
                self.write_number(n);
            } else if let Some(s) = val.as_string() {
                self.write_indent(depth);
                self.write_key(&key);
                self.buf.push_str(": ");
                self.write_string_value(&s);
            } else if Array::is_array(&val) {
                let arr = Array::from(&val);
                if arr.length() == 0 {
                    self.write_indent(depth);
                    self.write_key(&key);
                    self.buf.push_str(": []");
                } else {
                    self.encode_array_top(Some(&key), &arr, depth);
                }
            } else if val.is_object() {
                let inner = Object::from(val);
                let inner_keys = Object::keys(&inner);
                if inner_keys.length() == 0 {
                    self.write_indent(depth);
                    self.write_key(&key);
                    self.buf.push_str(": {}");
                } else {
                    self.write_indent(depth);
                    self.write_key(&key);
                    self.buf.push(':');
                    self.buf.push('\n');
                    self.encode_object(&inner, depth + 1);
                }
            }
        }
    }

    fn encode_array_top(&mut self, key: Option<&str>, arr: &Array, depth: usize) {
        let len = arr.length();

        // All primitives → inline
        if self.all_primitives(arr) {
            self.write_indent(depth);
            if let Some(k) = key { self.write_key(k); }
            self.buf.push('[');
            self.write_usize(len as usize);
            self.buf.push_str("]: ");
            for i in 0..len {
                if i > 0 { self.buf.push_str(", "); }
                self.write_primitive_value(&arr.get(i));
            }
            return;
        }

        // Tabular detection: all items are objects with same keys, all primitive values
        if let Some(fields) = self.extract_tabular_fields(arr) {
            self.write_indent(depth);
            if let Some(k) = key { self.write_key(k); }
            self.buf.push('[');
            self.write_usize(len as usize);
            self.buf.push_str("]{");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 { self.buf.push(','); }
                self.write_key(f);
            }
            self.buf.push_str("}:");
            for i in 0..len {
                self.buf.push('\n');
                self.write_indent(depth + 1);
                let item = Object::from(arr.get(i));
                for (j, f) in fields.iter().enumerate() {
                    if j > 0 { self.buf.push_str(", "); }
                    let v = Reflect::get(&item, &JsValue::from_str(f)).unwrap_or(JsValue::NULL);
                    self.write_primitive_value(&v);
                }
            }
            return;
        }

        // Expanded array with "- " prefix
        self.write_indent(depth);
        if let Some(k) = key { self.write_key(k); }
        self.buf.push('[');
        self.write_usize(len as usize);
        self.buf.push_str("]:");
        for i in 0..len {
            self.buf.push('\n');
            let item = arr.get(i);
            if item.is_null() || item.is_undefined() || item.as_bool().is_some()
                || item.as_f64().is_some() || item.as_string().is_some()
            {
                self.write_indent(depth + 1);
                self.buf.push_str("- ");
                self.write_primitive_value(&item);
            } else if Array::is_array(&item) {
                let inner = Array::from(&item);
                self.write_indent(depth + 1);
                self.buf.push_str("- ");
                if inner.length() == 0 {
                    self.buf.push_str("[]");
                } else {
                    self.buf.push('[');
                    self.write_usize(inner.length() as usize);
                    self.buf.push_str("]: ");
                    for j in 0..inner.length() {
                        if j > 0 { self.buf.push_str(", "); }
                        self.write_primitive_value(&inner.get(j));
                    }
                }
            } else if item.is_object() {
                let obj = Object::from(item);
                self.encode_object_as_list_item(&obj, depth + 1);
            }
        }
    }

    fn encode_object_as_list_item(&mut self, obj: &Object, depth: usize) {
        let keys = Object::keys(obj);
        let len = keys.length();
        if len == 0 {
            self.write_indent(depth);
            self.buf.push_str("- {}");
            return;
        }

        // First key on the "- " line
        let first_key: String = keys.get(0).as_string().unwrap_or_default();
        let first_val = Reflect::get(obj, &keys.get(0)).unwrap_or(JsValue::NULL);

        self.write_indent(depth);
        self.buf.push_str("- ");
        self.write_key(&first_key);

        if first_val.is_object() && !Array::is_array(&first_val)
            && !first_val.is_null() && !first_val.is_undefined()
        {
            let inner = Object::from(first_val);
            let inner_keys = Object::keys(&inner);
            if inner_keys.length() == 0 {
                self.buf.push_str(": {}");
            } else {
                self.buf.push(':');
                self.buf.push('\n');
                self.encode_object(&inner, depth + 2);
            }
        } else if Array::is_array(&first_val) {
            let arr = Array::from(&first_val);
            if arr.length() == 0 {
                self.buf.push_str(": []");
            } else if self.all_primitives(&arr) {
                self.buf.push('[');
                self.write_usize(arr.length() as usize);
                self.buf.push_str("]: ");
                for i in 0..arr.length() {
                    if i > 0 { self.buf.push_str(", "); }
                    self.write_primitive_value(&arr.get(i));
                }
            } else {
                self.buf.push('[');
                self.write_usize(arr.length() as usize);
                self.buf.push_str("]:");
                // expanded sub-array items would go here
            }
        } else {
            self.buf.push_str(": ");
            self.write_primitive_value(&first_val);
        }

        // Continuation fields at depth+1
        for i in 1..len {
            self.buf.push('\n');
            let key: String = keys.get(i).as_string().unwrap_or_default();
            let val = Reflect::get(obj, &keys.get(i)).unwrap_or(JsValue::NULL);

            if val.is_object() && !Array::is_array(&val)
                && !val.is_null() && !val.is_undefined()
            {
                let inner = Object::from(val);
                let inner_keys = Object::keys(&inner);
                if inner_keys.length() == 0 {
                    self.write_indent(depth + 1);
                    self.write_key(&key);
                    self.buf.push_str(": {}");
                } else {
                    self.write_indent(depth + 1);
                    self.write_key(&key);
                    self.buf.push(':');
                    self.buf.push('\n');
                    self.encode_object(&inner, depth + 2);
                }
            } else if Array::is_array(&val) {
                let arr = Array::from(&val);
                if arr.length() == 0 {
                    self.write_indent(depth + 1);
                    self.write_key(&key);
                    self.buf.push_str(": []");
                } else {
                    self.encode_array_top(Some(&key), &arr, depth + 1);
                }
            } else {
                self.write_indent(depth + 1);
                self.write_key(&key);
                self.buf.push_str(": ");
                self.write_primitive_value(&val);
            }
        }
    }

    // --- Tabular detection ---

    fn all_primitives(&self, arr: &Array) -> bool {
        for i in 0..arr.length() {
            let v = arr.get(i);
            if v.is_object() && !v.is_null() { return false; }
        }
        true
    }

    fn extract_tabular_fields(&self, arr: &Array) -> Option<Vec<String>> {
        if arr.length() == 0 { return None; }

        let first = arr.get(0);
        if !first.is_object() || first.is_null() || Array::is_array(&first) { return None; }
        let first_obj = Object::from(first);
        let first_keys = Object::keys(&first_obj);
        if first_keys.length() == 0 { return None; }

        let fields: Vec<String> = (0..first_keys.length())
            .map(|i| first_keys.get(i).as_string().unwrap_or_default())
            .collect();

        // Check all values in first object are primitive
        for f in &fields {
            let v = Reflect::get(&first_obj, &JsValue::from_str(f)).unwrap_or(JsValue::NULL);
            if v.is_object() && !v.is_null() { return None; }
        }

        // Check all remaining items have same keys with primitive values
        for i in 1..arr.length() {
            let item = arr.get(i);
            if !item.is_object() || item.is_null() || Array::is_array(&item) { return None; }
            let obj = Object::from(item);
            let obj_keys = Object::keys(&obj);
            if obj_keys.length() != first_keys.length() { return None; }
            for f in &fields {
                let v = Reflect::get(&obj, &JsValue::from_str(f)).unwrap_or(JsValue::UNDEFINED);
                if v.is_undefined() { return None; }
                if v.is_object() && !v.is_null() { return None; }
            }
        }

        Some(fields)
    }

    // --- Primitive writing ---

    fn write_primitive_value(&mut self, val: &JsValue) {
        if val.is_null() || val.is_undefined() {
            self.buf.push_str("null");
        } else if let Some(b) = val.as_bool() {
            self.buf.push_str(if b { "true" } else { "false" });
        } else if let Some(n) = val.as_f64() {
            self.write_number(n);
        } else if let Some(s) = val.as_string() {
            self.write_string_value(&s);
        } else {
            self.buf.push_str("null");
        }
    }

    fn write_number(&mut self, n: f64) {
        if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            let mut b = itoa::Buffer::new();
            self.buf.push_str(b.format(n as i64));
        } else {
            let mut b = ryu::Buffer::new();
            self.buf.push_str(b.format(n));
        }
    }

    fn format_number(&self, n: f64) -> String {
        if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            itoa::Buffer::new().format(n as i64).to_string()
        } else {
            ryu::Buffer::new().format(n).to_string()
        }
    }

    fn write_usize(&mut self, n: usize) {
        let mut b = itoa::Buffer::new();
        self.buf.push_str(b.format(n));
    }

    fn write_string_value(&mut self, s: &str) {
        if self.is_safe_unquoted(s) {
            self.buf.push_str(s);
        } else {
            self.buf.push('"');
            self.escape_string(s);
            self.buf.push('"');
        }
    }

    fn write_key(&mut self, key: &str) {
        if self.is_valid_unquoted_key(key) {
            self.buf.push_str(key);
        } else {
            self.buf.push('"');
            self.escape_string(key);
            self.buf.push('"');
        }
    }

    fn escape_string(&mut self, s: &str) {
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
            if last_flush < i {
                self.buf.push_str(&s[last_flush..i]);
            }
            self.buf.push_str(esc);
            last_flush = i + 1;
        }
        if last_flush < s.len() {
            self.buf.push_str(&s[last_flush..]);
        }
    }

    fn is_safe_unquoted(&self, s: &str) -> bool {
        if s.is_empty() { return false; }
        if matches!(s, "true" | "false" | "null") { return false; }
        let first = s.as_bytes()[0];
        if first.is_ascii_digit() || first == b'+' || first == b'-' || first == b'.' {
            return false;
        }
        for &b in s.as_bytes() {
            if UNSAFE_VALUE[b as usize] || b == b',' { return false; }
        }
        true
    }

    fn is_valid_unquoted_key(&self, key: &str) -> bool {
        if key.is_empty() { return false; }
        if key.as_bytes()[0] == b'#' { return false; }
        for &b in key.as_bytes() {
            if UNSAFE_KEY[b as usize] { return false; }
        }
        true
    }

    fn write_indent(&mut self, depth: usize) {
        let spaces = self.indent * depth;
        if spaces == 0 { return; }
        if spaces <= INDENT_SPACES.len() {
            self.buf.push_str(&INDENT_SPACES[..spaces]);
        } else {
            let full = spaces / INDENT_SPACES.len();
            let rem = spaces % INDENT_SPACES.len();
            for _ in 0..full { self.buf.push_str(INDENT_SPACES); }
            self.buf.push_str(&INDENT_SPACES[..rem]);
        }
    }
}

#[wasm_bindgen]
pub fn scon_encode(data: JsValue) -> String {
    JsEncoder::new(1).encode(&data)
}

#[wasm_bindgen]
pub fn scon_encode_indent(data: JsValue, indent: usize) -> String {
    JsEncoder::new(indent.max(1)).encode(&data)
}

#[wasm_bindgen]
pub fn scon_minify(input: &str) -> String {
    Minifier::minify(input)
}

#[wasm_bindgen]
pub fn scon_expand(input: &str, indent: usize) -> String {
    Minifier::expand(input, indent.max(1))
}
