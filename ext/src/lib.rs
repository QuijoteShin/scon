// ext/src/lib.rs
// SCON PHP Extension — native encode/decode via Rust
// Functions: scon_encode, scon_decode, scon_minify, scon_expand

#![cfg_attr(windows, feature(abi_vectorcall))]

use ext_php_rs::prelude::*;
use ext_php_rs::types::{Zval, ZendHashTable};
use indexmap::IndexMap;
use scon_core::Value;

// ============================================================================
// PHP Zval → scon::Value (recursive)
// ============================================================================

fn zval_to_value(zval: &Zval) -> Value {
    if zval.is_null() {
        return Value::Null;
    }
    if zval.is_bool() {
        return Value::Bool(zval.bool().unwrap_or(false));
    }
    if zval.is_long() {
        return Value::Integer(zval.long().unwrap_or(0));
    }
    if zval.is_double() {
        return Value::Float(zval.double().unwrap_or(0.0));
    }
    if zval.is_string() {
        return Value::String(zval.str().unwrap_or("").to_string());
    }
    if zval.is_array() {
        return ht_to_value(zval.array());
    }
    Value::Null
}

fn ht_to_value(ht: Option<&ZendHashTable>) -> Value {
    let ht = match ht {
        Some(h) => h,
        None => return Value::Array(vec![]),
    };

    if ht.is_empty() {
        return Value::Array(vec![]);
    }

    // Check if it's a sequential list (keys 0, 1, 2, ...)
    if ht.has_numerical_keys() {
        // Could still be sparse (e.g., [0 => a, 5 => b])
        // PHP's array_is_list checks packed sequential from 0
        let mut is_sequential = true;
        let mut idx: u64 = 0;
        for (key, _val) in ht.iter() {
            match key {
                ext_php_rs::types::ArrayKey::Long(n) if n as u64 == idx => idx += 1,
                _ => { is_sequential = false; break; }
            }
        }

        if is_sequential {
            let mut arr = Vec::with_capacity(ht.len());
            for (_key, val) in ht.iter() {
                arr.push(zval_to_value(val));
            }
            return Value::Array(arr);
        }
    }

    // Associative array → Object
    let mut map = IndexMap::with_capacity(ht.len());
    for (key, val) in ht.iter() {
        let k = match key {
            ext_php_rs::types::ArrayKey::Long(n) => n.to_string(),
            ext_php_rs::types::ArrayKey::String(s) => s.to_string(),
        };
        map.insert(k, zval_to_value(val));
    }
    Value::Object(map)
}

// ============================================================================
// scon::Value → PHP Zval (recursive)
// ============================================================================

fn value_to_zval(value: &Value) -> Zval {
    match value {
        Value::Null => {
            let mut z = Zval::new();
            z.set_null();
            z
        }
        Value::Bool(b) => {
            let mut z = Zval::new();
            z.set_bool(*b);
            z
        }
        Value::Integer(i) => {
            let mut z = Zval::new();
            z.set_long(*i);
            z
        }
        Value::Float(f) => {
            let mut z = Zval::new();
            z.set_double(*f);
            z
        }
        Value::String(s) => {
            let mut z = Zval::new();
            z.set_string(s, false).ok();
            z
        }
        Value::Array(arr) => {
            let mut ht = ZendHashTable::with_capacity(arr.len() as u32);
            for item in arr {
                let val = value_to_zval(item);
                ht.push(val).ok();
            }
            let mut z = Zval::new();
            z.set_hashtable(ht);
            z
        }
        Value::Object(map) => {
            let mut ht = ZendHashTable::with_capacity(map.len() as u32);
            for (key, val) in map {
                let v = value_to_zval(val);
                ht.insert(key, v).ok();
            }
            let mut z = Zval::new();
            z.set_hashtable(ht);
            z
        }
    }
}

// ============================================================================
// PHP functions
// ============================================================================

/// Encode PHP data to SCON string (indent=1 default)
#[php_function]
pub fn scon_encode(data: &Zval) -> String {
    let value = zval_to_value(data);
    scon_core::encode(&value)
}

/// Encode PHP data to SCON string with custom indent
#[php_function]
pub fn scon_encode_indent(data: &Zval, indent: i64) -> String {
    let value = zval_to_value(data);
    scon_core::encode_with_indent(&value, indent.max(1) as usize)
}

/// Decode SCON string to PHP array
#[php_function]
pub fn scon_decode(scon_string: &str) -> PhpResult<Zval> {
    match scon_core::decode(scon_string) {
        Ok(value) => Ok(value_to_zval(&value)),
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
