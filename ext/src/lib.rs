// ext/src/lib.rs
// SCON PHP Extension — zero-intermediate encode/decode via Rust (ext-php-rs FFI)
// Exposes: scon_encode, scon_encode_indent, scon_decode, scon_minify, scon_expand
//
// P19 architecture: eliminates scon_core::Value as intermediary.
//   Decode: SCON string -> TapeDecoder -> Vec<Node> -> walk tape -> emit Zvals directly
//   Encode: walk Zval/ZendHashTable -> write SCON string directly to buffer
//
// This is the same pattern that makes json_decode fast in C: parse -> emit PHP types
// in one logical pass, without constructing an intermediate AST.

#![cfg_attr(windows, feature(abi_vectorcall))]

use ext_php_rs::prelude::*;
use ext_php_rs::types::{Zval, ZendHashTable};
use scon_core::tape::{TapeDecoder, Node};

// ============================================================================
// Tape -> PHP Zval (decode: zero-intermediate)
// ============================================================================

// Walk tape nodes emitting Zvals directly — no Value, no IndexMap, no CompactString
fn tape_node_to_zval(nodes: &[Node], pos: &mut usize) -> Zval {
    if *pos >= nodes.len() {
        let mut z = Zval::new();
        z.set_null();
        return z;
    }

    match &nodes[*pos] {
        Node::Null => {
            *pos += 1;
            let mut z = Zval::new();
            z.set_null();
            z
        }
        Node::Bool(b) => {
            let b = *b;
            *pos += 1;
            let mut z = Zval::new();
            z.set_bool(b);
            z
        }
        Node::Integer(i) => {
            let i = *i;
            *pos += 1;
            let mut z = Zval::new();
            z.set_long(i);
            z
        }
        Node::Float(f) => {
            let f = *f;
            *pos += 1;
            let mut z = Zval::new();
            z.set_double(f);
            z
        }
        Node::String(s) => {
            let s = *s;
            *pos += 1;
            let mut z = Zval::new();
            z.set_string(s, false).ok();
            z
        }
        Node::Array(count) => {
            let count = *count;
            *pos += 1;
            let mut ht = ZendHashTable::with_capacity(count as u32);
            for _ in 0..count {
                let val = tape_node_to_zval(nodes, pos);
                ht.push(val).ok();
            }
            let mut z = Zval::new();
            z.set_hashtable(ht);
            z
        }
        Node::Object(count) => {
            let count = *count;
            *pos += 1;
            let mut ht = ZendHashTable::with_capacity(count as u32);
            for _ in 0..count {
                // Next node must be Key
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

                let val = tape_node_to_zval(nodes, pos);

                // PHP casts numeric string keys to integers ("200" -> 200)
                if let Ok(n) = key.parse::<u64>() {
                    ht.insert_at_index(n, val).ok();
                } else {
                    ht.insert(key, val).ok();
                }
            }
            let mut z = Zval::new();
            z.set_hashtable(ht);
            z
        }
        Node::Key(_) => {
            // Should not appear at top level — skip
            *pos += 1;
            let mut z = Zval::new();
            z.set_null();
            z
        }
    }
}

// ============================================================================
// PHP Zval -> SCON string (encode: zero-intermediate)
// ============================================================================

// Lookup tables for quoting — same as scon_core::encoder
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

const INDENT_SPACES: &str = "                                                                ";

struct ZvalEncoder {
    indent: usize,
}

impl ZvalEncoder {
    fn new(indent: usize) -> Self {
        Self { indent: indent.max(1) }
    }

    fn encode(&self, data: &Zval) -> String {
        let mut buf = String::with_capacity(1024);
        if data.is_array() {
            if let Some(ht) = data.array() {
                if ht.is_empty() {
                    // Detect if it should be {} or [] — empty PHP arrays are ambiguous
                    // Default to [] for consistency
                    buf.push_str("[]");
                } else if self.is_sequential(ht) {
                    self.encode_array_ht(None, ht, 0, &mut buf);
                } else {
                    self.encode_object_ht(ht, 0, &mut buf);
                }
            } else {
                buf.push_str("[]");
            }
        } else {
            self.write_primitive_zval(data, &mut buf);
        }
        buf
    }

    // Check if hash table is sequential 0,1,2,...
    fn is_sequential(&self, ht: &ZendHashTable) -> bool {
        if !ht.has_numerical_keys() { return false; }
        let mut idx: u64 = 0;
        for (key, _) in ht.iter() {
            match key {
                ext_php_rs::types::ArrayKey::Long(n) if n as u64 == idx => idx += 1,
                _ => return false,
            }
        }
        true
    }

    // Check if a Zval is a primitive (not array/object)
    fn zval_is_primitive(zval: &Zval) -> bool {
        !zval.is_array()
    }

    fn encode_object_ht(&self, ht: &ZendHashTable, depth: usize, buf: &mut String) {
        let mut first = true;
        for (key, val) in ht.iter() {
            if !first { buf.push('\n'); }
            first = false;

            let key_str = match &key {
                ext_php_rs::types::ArrayKey::Long(n) => {
                    // Format inline to avoid allocation where possible
                    let mut itoa_buf = itoa::Buffer::new();
                    let s = itoa_buf.format(*n as i64);
                    self.write_indent(depth, buf);
                    self.write_key(s, buf);
                    self.encode_value_after_key(val, depth, buf);
                    continue;
                }
                ext_php_rs::types::ArrayKey::String(s) => s.to_string(),
            };

            self.write_indent(depth, buf);
            self.write_key(&key_str, buf);
            self.encode_value_after_key(val, depth, buf);
        }
    }

    fn encode_value_after_key(&self, val: &Zval, depth: usize, buf: &mut String) {
        if val.is_array() {
            if let Some(inner_ht) = val.array() {
                if inner_ht.is_empty() {
                    buf.push_str(": []");
                } else if self.is_sequential(inner_ht) {
                    buf.push_str("");
                    self.encode_array_ht(None, inner_ht, depth, buf);
                } else {
                    // Nested object
                    buf.push(':');
                    buf.push('\n');
                    self.encode_object_ht(inner_ht, depth + 1, buf);
                }
            } else {
                buf.push_str(": []");
            }
        } else {
            buf.push_str(": ");
            self.write_primitive_zval(val, buf);
        }
    }

    fn encode_array_ht(&self, key_already_written: Option<&str>, ht: &ZendHashTable, depth: usize, buf: &mut String) {
        let len = ht.len();

        if len == 0 {
            if key_already_written.is_none() {
                buf.push_str("[]");
            }
            return;
        }

        // Collect values to check type patterns
        let vals: Vec<&Zval> = ht.iter().map(|(_, v)| v).collect();

        // All primitives -> inline array
        if vals.iter().all(|v| Self::zval_is_primitive(v)) {
            buf.push('[');
            self.write_usize(len, buf);
            buf.push_str("]: ");
            for (i, v) in vals.iter().enumerate() {
                if i > 0 { buf.push_str(", "); }
                self.write_primitive_zval(v, buf);
            }
            return;
        }

        // Check for tabular: all items are assoc arrays with same string keys, all values primitive
        if let Some(fields) = self.extract_tabular_fields(&vals) {
            buf.push('[');
            self.write_usize(len, buf);
            buf.push_str("]{");
            for (i, f) in fields.iter().enumerate() {
                if i > 0 { buf.push(','); }
                self.write_key(f, buf);
            }
            buf.push_str("}:");
            for v in &vals {
                if let Some(obj_ht) = v.array() {
                    buf.push('\n');
                    self.write_indent(depth + 1, buf);
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 { buf.push_str(", "); }
                        // Look up field value by iterating (ext-php-rs get() not available by &str)
                        let mut found = false;
                        for (k, fv) in obj_ht.iter() {
                            let k_str = match &k {
                                ext_php_rs::types::ArrayKey::String(s) => s.as_str(),
                                ext_php_rs::types::ArrayKey::Long(_) => continue,
                            };
                            if k_str == *f {
                                self.write_primitive_zval(fv, buf);
                                found = true;
                                break;
                            }
                        }
                        if !found { buf.push_str("null"); }
                    }
                }
            }
            return;
        }

        // Mixed / expanded array
        buf.push('[');
        self.write_usize(len, buf);
        buf.push_str("]:");
        for v in &vals {
            buf.push('\n');
            if Self::zval_is_primitive(v) {
                self.write_indent(depth + 1, buf);
                buf.push_str("- ");
                self.write_primitive_zval(v, buf);
            } else if v.is_array() {
                if let Some(inner_ht) = v.array() {
                    if inner_ht.is_empty() {
                        self.write_indent(depth + 1, buf);
                        buf.push_str("- {}");
                    } else if self.is_sequential(inner_ht) {
                        self.write_indent(depth + 1, buf);
                        buf.push_str("- ");
                        self.encode_array_ht(None, inner_ht, depth + 1, buf);
                    } else {
                        self.encode_object_as_list_item(inner_ht, depth + 1, buf);
                    }
                }
            }
        }
    }

    // Tabular detection: all items are assoc arrays with identical string keys, all values primitive
    fn extract_tabular_fields<'a>(&self, vals: &[&'a Zval]) -> Option<Vec<String>> {
        if vals.is_empty() { return None; }

        let first_ht = vals[0].array()?;
        if first_ht.is_empty() { return None; }
        if self.is_sequential(first_ht) { return None; }

        // Extract keys from first item, verify all values are primitive
        let mut fields: Vec<String> = Vec::new();
        for (k, v) in first_ht.iter() {
            if !Self::zval_is_primitive(v) { return None; }
            match k {
                ext_php_rs::types::ArrayKey::String(s) => fields.push(s.to_string()),
                _ => return None,
            }
        }

        if fields.is_empty() { return None; }

        // Verify remaining items have same keys in same order
        for val in &vals[1..] {
            let ht = val.array()?;
            if ht.len() != fields.len() { return None; }
            let mut idx = 0;
            for (k, v) in ht.iter() {
                if !Self::zval_is_primitive(v) { return None; }
                match k {
                    ext_php_rs::types::ArrayKey::String(s) => {
                        if idx >= fields.len() || s.as_str() != fields[idx] { return None; }
                    }
                    _ => return None,
                }
                idx += 1;
            }
        }

        Some(fields)
    }

    fn encode_object_as_list_item(&self, ht: &ZendHashTable, depth: usize, buf: &mut String) {
        if ht.is_empty() {
            self.write_indent(depth, buf);
            buf.push_str("- {}");
            return;
        }

        let mut iter = ht.iter();
        let (first_key, first_val) = match iter.next() {
            Some(kv) => kv,
            None => return,
        };

        self.write_indent(depth, buf);
        buf.push_str("- ");
        let first_key_str = match &first_key {
            ext_php_rs::types::ArrayKey::Long(n) => n.to_string(),
            ext_php_rs::types::ArrayKey::String(s) => s.to_string(),
        };
        self.write_key(&first_key_str, buf);

        if first_val.is_array() {
            if let Some(inner_ht) = first_val.array() {
                if inner_ht.is_empty() {
                    buf.push_str(": {}");
                } else if self.is_sequential(inner_ht) {
                    self.encode_array_ht(None, inner_ht, depth + 1, buf);
                } else {
                    buf.push(':');
                    buf.push('\n');
                    self.encode_object_ht(inner_ht, depth + 2, buf);
                }
            } else {
                buf.push_str(": []");
            }
        } else {
            buf.push_str(": ");
            self.write_primitive_zval(first_val, buf);
        }

        // Continuation fields
        for (key, val) in iter {
            buf.push('\n');
            let key_str = match &key {
                ext_php_rs::types::ArrayKey::Long(n) => n.to_string(),
                ext_php_rs::types::ArrayKey::String(s) => s.to_string(),
            };
            self.write_indent(depth + 1, buf);
            self.write_key(&key_str, buf);

            if val.is_array() {
                if let Some(inner_ht) = val.array() {
                    if inner_ht.is_empty() {
                        buf.push_str(": []");
                    } else if self.is_sequential(inner_ht) {
                        self.encode_array_ht(None, inner_ht, depth + 1, buf);
                    } else {
                        buf.push(':');
                        buf.push('\n');
                        self.encode_object_ht(inner_ht, depth + 2, buf);
                    }
                } else {
                    buf.push_str(": []");
                }
            } else {
                buf.push_str(": ");
                self.write_primitive_zval(val, buf);
            }
        }
    }

    // --- Primitive writing ---

    fn write_primitive_zval(&self, zval: &Zval, buf: &mut String) {
        if zval.is_null() {
            buf.push_str("null");
        } else if zval.is_bool() {
            buf.push_str(if zval.bool().unwrap_or(false) { "true" } else { "false" });
        } else if zval.is_long() {
            let mut itoa_buf = itoa::Buffer::new();
            buf.push_str(itoa_buf.format(zval.long().unwrap_or(0)));
        } else if zval.is_double() {
            let mut ryu_buf = ryu::Buffer::new();
            buf.push_str(ryu_buf.format(zval.double().unwrap_or(0.0)));
        } else if zval.is_string() {
            let s = zval.str().unwrap_or("");
            self.write_string(s, buf);
        } else {
            buf.push_str("null");
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
                buf.push_str(&s[last_flush..i]);
            }
            buf.push_str(esc);
            last_flush = i + 1;
        }
        if last_flush < s.len() {
            buf.push_str(&s[last_flush..]);
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
            if UNSAFE_VALUE[b as usize] || b == b',' {
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

    #[inline]
    fn write_usize(&self, n: usize, buf: &mut String) {
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(n));
    }

    fn write_indent(&self, depth: usize, buf: &mut String) {
        let spaces = self.indent * depth;
        if spaces == 0 { return; }
        if spaces <= INDENT_SPACES.len() {
            buf.push_str(&INDENT_SPACES[..spaces]);
        } else {
            let full = spaces / INDENT_SPACES.len();
            let rem = spaces % INDENT_SPACES.len();
            for _ in 0..full { buf.push_str(INDENT_SPACES); }
            buf.push_str(&INDENT_SPACES[..rem]);
        }
    }
}

// ============================================================================
// PHP functions
// ============================================================================

/// Encode PHP data to SCON string (indent=1 default) — zero-intermediate path
#[php_function]
pub fn scon_encode(data: &Zval) -> String {
    ZvalEncoder::new(1).encode(data)
}

/// Encode PHP data to SCON string with custom indent — zero-intermediate path
#[php_function]
pub fn scon_encode_indent(data: &Zval, indent: i64) -> String {
    ZvalEncoder::new(indent.max(1) as usize).encode(data)
}

/// Decode SCON string to PHP array — tape -> Zval direct, no Value intermediate
#[php_function]
pub fn scon_decode(scon_string: &str) -> PhpResult<Zval> {
    let mut decoder = TapeDecoder::new();
    match decoder.decode(scon_string) {
        Ok(tape) => {
            let mut pos = 0;
            Ok(tape_node_to_zval(&tape.nodes, &mut pos))
        }
        Err(e) => Err(PhpException::default(format!("SCON decode error: {}", e))),
    }
}

/// Minify SCON string (semicolon-delimited single line)
#[php_function]
pub fn scon_minify(scon_string: &str) -> String {
    scon_core::minify(scon_string)
}

/// Expand minified SCON to indented format
#[php_function]
pub fn scon_expand(minified: &str, indent: Option<i64>) -> String {
    scon_core::expand(minified, indent.unwrap_or(1).max(1) as usize)
}

// Module registration
#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
}
