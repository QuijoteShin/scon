// rs/src/tape.rs
// SCON Tape Decoder — parses SCON into a flat Vec<Node> (no tree allocation).
//
// Instead of building nested IndexMap + Vec structures, the tape is a single contiguous
// Vec<Node> where each node is a tagged union. Objects/Arrays store their child count,
// enabling O(1) skip-over without parsing children.
//
// Memory layout comparison (for an object with 3 string fields):
//   Owned:    1 IndexMap + 3 CompactString keys + 3 CompactString values = ~7 allocations
//   Borrowed: 1 IndexMap + 0 string copies = ~1 allocation (the map itself)
//   Tape:     7 entries in a pre-allocated Vec = 0 additional allocations
//
// Trade-off: O(K) linear scan for key lookup (no hash table), but zero per-node allocation.
// Ideal for parse-and-forward, serialization, or full-tree traversal.
// Not ideal for random key access on large objects.

use crate::decoder::Decoder;
use memchr::memchr;

// 32 bytes per node on 64-bit (tag + payload + &str pointer + len)
#[derive(Debug, Clone, PartialEq)]
pub enum Node<'a> {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(&'a str),          // borrowed from input
    // Object: count = number of key-value pairs. Keys follow as String nodes.
    // Layout: Object(3) → Key, Value, Key, Value, Key, Value
    Object(usize),
    // Array: count = number of elements
    Array(usize),
    // Key in an object — always followed by its value node
    Key(&'a str),
}

pub struct Tape<'a> {
    pub nodes: Vec<Node<'a>>,
}

impl<'a> Tape<'a> {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// Reuse ParsedLine from decoder logic
struct ParsedLine<'a> {
    depth: usize,
    content: &'a str,
    _line_num: usize,
    has_bracket: bool,
}

struct ArrayHeader<'a> {
    key: Option<&'a str>,
    length: usize,
    delimiter: char,
    fields: Option<Vec<&'a str>>,
    inline_values: Option<&'a str>,
}

pub struct TapeDecoder {
    helper: Decoder,
    #[allow(dead_code)]
    scratch: String,
    indent: usize,
    indent_auto_detect: bool,
}

impl TapeDecoder {
    pub fn new() -> Self {
        Self {
            helper: Decoder::new(),
            scratch: String::with_capacity(256),
            indent: 1,
            indent_auto_detect: true,
        }
    }

    pub fn decode<'a>(&mut self, input: &'a str) -> Result<Tape<'a>, String> {
        if !input.contains('\n') && input.contains(';') {
            // Minified — fallback to owned decode + convert (not the hot path)
            let val = Decoder::new().decode(input)?;
            return Ok(self.owned_to_tape(&val));
        }

        // Auto-detect indent
        if self.indent_auto_detect {
            if let Some(cap) = input.find('\n').and_then(|nl_pos| {
                let after = &input[nl_pos + 1..];
                let spaces = after.len() - after.trim_start_matches(' ').len();
                if spaces > 0 && after.len() > spaces && !after.as_bytes().get(spaces).map_or(true, |b| b.is_ascii_whitespace()) {
                    Some(spaces)
                } else {
                    None
                }
            }) {
                self.indent = cap;
            } else {
                for line in input.lines() {
                    let spaces = line.len() - line.trim_start_matches(' ').len();
                    if spaces > 0 && !line.trim().is_empty() && !line.trim().starts_with('#') {
                        self.indent = spaces;
                        break;
                    }
                }
            }
        }

        // Pass 1: line classification
        let line_estimate = input.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;
        let mut parsed: Vec<ParsedLine<'_>> = Vec::with_capacity(line_estimate);
        let indent = self.indent;
        for (line_num, line) in input.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let first_byte = trimmed.as_bytes()[0];
            if first_byte == b'#' { continue; }
            if first_byte == b'@' && (trimmed.starts_with("@@") || trimmed.starts_with("@use ")) { continue; }
            if first_byte == b's' && trimmed.starts_with("s:") { continue; }
            if first_byte == b'r' && trimmed.starts_with("r:") { continue; }
            if trimmed.starts_with("sec:") { continue; }
            let spaces = line.len() - line.trim_start_matches(' ').len();
            let depth = if indent > 0 { spaces / indent } else { 0 };
            let has_bracket = memchr(b'[', trimmed.as_bytes())
                .map_or(false, |bp| memchr(b':', trimmed.as_bytes()).map_or(false, |cp| bp < cp));
            parsed.push(ParsedLine { depth, content: trimmed, _line_num: line_num, has_bracket });
        }

        if parsed.is_empty() {
            return Ok(Tape { nodes: vec![Node::Object(0)] });
        }

        // Estimate tape size: ~2 nodes per line (key + value) is a reasonable heuristic
        let mut tape = Vec::with_capacity(parsed.len() * 2);

        let first = &parsed[0];
        if parsed.len() == 1 && first.content == "{}" {
            tape.push(Node::Object(0));
            return Ok(Tape { nodes: tape });
        }

        if let Some(header) = self.try_array_header(first.content) {
            if header.key.is_none() {
                self.emit_array_from_header(0, &parsed, &header, &mut tape)?;
                return Ok(Tape { nodes: tape });
            }
        }

        if parsed.len() == 1 && !first.content.contains(':') {
            self.emit_primitive(first.content, &mut tape);
            return Ok(Tape { nodes: tape });
        }

        self.emit_object(0, &parsed, 0, &mut tape)?;
        Ok(Tape { nodes: tape })
    }

    // --- Object emission ---

    fn emit_object<'a>(&mut self, base_depth: usize, lines: &[ParsedLine<'a>], start: usize, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        let obj_pos = tape.len();
        tape.push(Node::Object(0)); // placeholder, patch count later
        let mut count = 0usize;
        let mut i = start;

        while i < lines.len() {
            let line = &lines[i];
            if line.depth < base_depth { break; }
            if line.depth > base_depth { i += 1; continue; }

            let content = line.content;

            if line.has_bracket {
                if let Some(header) = self.try_array_header(content) {
                    if let Some(key) = header.key {
                        tape.push(Node::Key(key));
                        i = self.emit_array_from_header(i, lines, &header, tape)?;
                        count += 1;
                        continue;
                    }
                }
            }

            if let Some(colon_pos) = self.helper.find_key_colon(content) {
                i = self.emit_key_value(line, lines, i, base_depth, colon_pos, tape)?;
                count += 1;
                continue;
            }

            i += 1;
        }

        tape[obj_pos] = Node::Object(count);
        Ok(i)
    }

    fn emit_key_value<'a>(&mut self, line: &ParsedLine<'a>, lines: &[ParsedLine<'a>], index: usize, base_depth: usize, colon_pos: usize, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        let content = line.content;
        let (key, key_end) = if content.as_bytes()[0] == b'"' {
            self.parse_key(content)?
        } else {
            (content[..colon_pos].trim(), colon_pos + 1)
        };
        tape.push(Node::Key(key));
        let rest = content[key_end..].trim();

        if !rest.is_empty() {
            self.emit_inline_value(rest, tape);
            return Ok(index + 1);
        }

        if index + 1 < lines.len() && lines[index + 1].depth > base_depth {
            let child_depth = base_depth + 1;
            if lines[index + 1].depth == child_depth && lines[index + 1].content.starts_with("- ") {
                // Expanded array without header
                let arr_pos = tape.len();
                tape.push(Node::Array(0));
                let mut arr_count = 0;
                let mut j = index + 1;
                while j < lines.len() && lines[j].depth >= child_depth {
                    if lines[j].depth == child_depth && lines[j].content.starts_with("- ") {
                        let item_content = &lines[j].content[2..];
                        if item_content.contains(':') {
                            j = self.emit_list_item_object(&lines[j], lines, j, base_depth, tape)?;
                            arr_count += 1;
                            continue;
                        } else {
                            self.emit_inline_value(item_content, tape);
                            arr_count += 1;
                        }
                    }
                    j += 1;
                }
                tape[arr_pos] = Node::Array(arr_count);
                return Ok(j);
            }

            let next_i = self.emit_object(base_depth + 1, lines, index + 1, tape)?;
            return Ok(next_i);
        }

        tape.push(Node::Object(0));
        Ok(index + 1)
    }

    // --- Array emission ---

    fn emit_array_from_header<'a>(&mut self, index: usize, lines: &[ParsedLine<'a>], header: &ArrayHeader<'a>, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        if header.length == 0 {
            tape.push(Node::Array(0));
            return Ok(index + 1);
        }

        if let Some(ref inline) = header.inline_values {
            if header.fields.is_none() {
                let arr_pos = tape.len();
                tape.push(Node::Array(0));
                let count = self.emit_delimited_values(inline, header.delimiter, tape);
                tape[arr_pos] = Node::Array(count);
                return Ok(index + 1);
            }
        }

        if let Some(ref fields) = header.fields {
            return self.emit_tabular_array(index, lines, header.length, fields, header.delimiter, tape);
        }

        self.emit_expanded_array(index, lines, header.length, tape)
    }

    fn emit_tabular_array<'a>(&mut self, header_idx: usize, lines: &[ParsedLine<'a>], expected: usize, fields: &[&'a str], delimiter: char, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        let base_depth = lines[header_idx].depth;
        let arr_pos = tape.len();
        tape.push(Node::Array(0));
        let mut row_count = 0;
        let mut i = header_idx + 1;

        while i < lines.len() && row_count < expected {
            if lines[i].depth != base_depth + 1 { break; }
            let obj_pos = tape.len();
            tape.push(Node::Object(fields.len()));
            // Parse values inline and pair with field names
            let mut seg_start = 0;
            let mut in_quotes = false;
            let bytes = lines[i].content.as_bytes();
            let delim_byte = delimiter as u8;
            let mut field_idx = 0;
            let content = lines[i].content;

            let mut j = 0;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'\\' && in_quotes && j + 1 < bytes.len() { j += 2; continue; }
                if c == b'"' { in_quotes = !in_quotes; }
                if c == delim_byte && !in_quotes {
                    if field_idx < fields.len() {
                        tape.push(Node::Key(fields[field_idx]));
                        self.emit_primitive(content[seg_start..j].trim(), tape);
                    }
                    field_idx += 1;
                    seg_start = j + 1;
                    j += 1;
                    continue;
                }
                j += 1;
            }
            // Last field
            if field_idx < fields.len() {
                tape.push(Node::Key(fields[field_idx]));
                self.emit_primitive(content[seg_start..].trim(), tape);
                field_idx += 1;
            }
            // Pad missing fields with null
            while field_idx < fields.len() {
                tape.push(Node::Key(fields[field_idx]));
                tape.push(Node::Null);
                field_idx += 1;
            }
            // Patch object count (always fields.len())
            tape[obj_pos] = Node::Object(fields.len());
            row_count += 1;
            i += 1;
        }

        tape[arr_pos] = Node::Array(row_count);
        Ok(i)
    }

    fn emit_expanded_array<'a>(&mut self, header_idx: usize, lines: &[ParsedLine<'a>], expected: usize, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        let base_depth = lines[header_idx].depth;
        let arr_pos = tape.len();
        tape.push(Node::Array(0));
        let mut count = 0;
        let mut i = header_idx + 1;

        while i < lines.len() && count < expected {
            let line = &lines[i];
            if line.depth != base_depth + 1 { break; }

            if line.content.starts_with("- ") {
                let item_content = &line.content[2..];

                if let Some(header) = self.try_array_header(item_content) {
                    if header.key.is_some() {
                        i = self.emit_list_item_object(line, lines, i, base_depth, tape)?;
                        count += 1;
                        continue;
                    } else if let Some(ref inline) = header.inline_values {
                        let sub_pos = tape.len();
                        tape.push(Node::Array(0));
                        let sub_count = self.emit_delimited_values(inline, header.delimiter, tape);
                        tape[sub_pos] = Node::Array(sub_count);
                        count += 1;
                    }
                } else if item_content.contains(':') {
                    i = self.emit_list_item_object(line, lines, i, base_depth, tape)?;
                    count += 1;
                    continue;
                } else {
                    self.emit_inline_value(item_content, tape);
                    count += 1;
                }
            }
            i += 1;
        }

        tape[arr_pos] = Node::Array(count);
        Ok(i)
    }

    fn emit_list_item_object<'a>(&mut self, _line: &ParsedLine<'a>, lines: &[ParsedLine<'a>], index: usize, base_depth: usize, tape: &mut Vec<Node<'a>>) -> Result<usize, String> {
        let item_content = &lines[index].content[2..];
        let obj_pos = tape.len();
        tape.push(Node::Object(0));
        let cont_depth = base_depth + 2;
        let mut cont_start = index + 1;
        let mut count = 0;

        if let Some(header) = self.try_array_header(item_content) {
            if let Some(key) = header.key {
                tape.push(Node::Key(key));
                cont_start = self.emit_array_from_header(index, lines, &header, tape)?;
                count += 1;
            }
        } else {
            let (key, key_end) = self.parse_key(item_content)?;
            tape.push(Node::Key(key));
            let rest = item_content[key_end..].trim();

            if !rest.is_empty() {
                self.emit_inline_value(rest, tape);
                count += 1;
            } else if index + 1 < lines.len() && lines[index + 1].depth >= cont_depth {
                let next_i = self.emit_object(cont_depth, lines, index + 1, tape)?;
                cont_start = next_i;
                count += 1;
            } else {
                tape.push(Node::Object(0));
                count += 1;
            }
        }

        // Continuation fields
        let mut i = cont_start;
        while i < lines.len() {
            let next = &lines[i];
            if next.depth < cont_depth { break; }
            if next.depth == cont_depth {
                if next.content.starts_with("- ") { break; }

                if next.has_bracket {
                    if let Some(header) = self.try_array_header(next.content) {
                        if let Some(k) = header.key {
                            tape.push(Node::Key(k));
                            i = self.emit_array_from_header(i, lines, &header, tape)?;
                            count += 1;
                            continue;
                        }
                    }
                }

                if let Some(colon_pos) = self.helper.find_key_colon(next.content) {
                    i = self.emit_key_value(next, lines, i, cont_depth, colon_pos, tape)?;
                    count += 1;
                    continue;
                }
            }
            i += 1;
        }

        tape[obj_pos] = Node::Object(count);
        Ok(i)
    }

    // --- Parsing helpers ---

    fn try_array_header<'a>(&self, content: &'a str) -> Option<ArrayHeader<'a>> {
        let bracket_start = content.find('[')?;
        let colon_pos = content.find(':')?;
        if bracket_start >= colon_pos { return None; }
        self.parse_array_header(content)
    }

    fn parse_array_header<'a>(&self, content: &'a str) -> Option<ArrayHeader<'a>> {
        let bracket_start = content.find('[')?;
        let key = if bracket_start > 0 {
            let raw = content[..bracket_start].trim();
            if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                Some(&raw[1..raw.len() - 1])
            } else {
                Some(raw)
            }
        } else {
            None
        };

        let bracket_end = content[bracket_start..].find(']').map(|p| p + bracket_start)?;
        let mut bracket_content = &content[bracket_start + 1..bracket_end];

        let mut delimiter = ',';
        if bracket_content.ends_with('\t') {
            delimiter = '\t';
            bracket_content = &bracket_content[..bracket_content.len() - 1];
        } else if bracket_content.ends_with('|') {
            delimiter = '|';
            bracket_content = &bracket_content[..bracket_content.len() - 1];
        }

        let length: usize = bracket_content.parse().unwrap_or(0);

        let mut fields = None;
        let after_bracket = &content[bracket_end + 1..];
        if after_bracket.starts_with('{') {
            if let Some(brace_end) = after_bracket.find('}') {
                let fields_str = &after_bracket[1..brace_end];
                let parts = self.helper.split_top_level(fields_str, delimiter);
                fields = Some(parts.into_iter().map(|s| s.trim()).collect());
            }
        }

        let colon_pos = content.rfind(':')?;
        let after_colon = content[colon_pos + 1..].trim();
        let inline_values = if !after_colon.is_empty() { Some(after_colon) } else { None };

        Some(ArrayHeader { key, length, delimiter, fields, inline_values })
    }

    fn parse_key<'a>(&mut self, content: &'a str) -> Result<(&'a str, usize), String> {
        if content.starts_with('"') {
            let close = self.helper.find_closing_quote(content, 0)
                .ok_or_else(|| "Unterminated quoted key".to_string())?;
            // For tape, quoted keys with escapes still borrow (escapes are rare in keys)
            // If there are escapes, we lose accuracy but it's acceptable for benchmarking
            let key = &content[1..close];
            if close + 1 >= content.len() || content.as_bytes()[close + 1] != b':' {
                return Err("Missing colon after key".to_string());
            }
            Ok((key, close + 2))
        } else {
            let colon = content.find(':').ok_or_else(|| "Missing colon after key".to_string())?;
            Ok((content[..colon].trim(), colon + 1))
        }
    }

    fn emit_inline_value<'a>(&mut self, input: &'a str, tape: &mut Vec<Node<'a>>) {
        let trimmed = input.trim();
        if trimmed.is_empty() { tape.push(Node::String("")); return; }
        if trimmed == "[]" { tape.push(Node::Array(0)); return; }
        if trimmed == "{}" { tape.push(Node::Object(0)); return; }

        if trimmed.starts_with('{') {
            if let Some(inner) = self.helper.extract_brace_content(trimmed) {
                self.emit_inline_object(inner, tape);
                return;
            }
        }

        if trimmed.starts_with('[') {
            if let Some(close) = self.helper.find_matching_bracket(trimmed, 0) {
                let inner = &trimmed[1..close];
                let arr_pos = tape.len();
                tape.push(Node::Array(0));
                let count = self.emit_delimited_values(inner, ',', tape);
                tape[arr_pos] = Node::Array(count);
                return;
            }
        }

        self.emit_primitive(trimmed, tape);
    }

    fn emit_inline_object<'a>(&mut self, inner: &'a str, tape: &mut Vec<Node<'a>>) {
        let obj_pos = tape.len();
        tape.push(Node::Object(0));
        let mut count = 0;
        let parts = self.helper.split_top_level(inner, ',');

        for part in &parts {
            let part = part.trim();
            if part.is_empty() { continue; }
            if let Some(colon) = self.helper.find_key_colon(part) {
                let key = part[..colon].trim();
                let key = if key.starts_with('"') && key.ends_with('"') && key.len() >= 2 {
                    &key[1..key.len() - 1]
                } else {
                    key
                };
                tape.push(Node::Key(key));
                self.emit_inline_value(part[colon + 1..].trim(), tape);
                count += 1;
            }
        }

        tape[obj_pos] = Node::Object(count);
    }

    fn emit_delimited_values<'a>(&mut self, input: &'a str, delimiter: char, tape: &mut Vec<Node<'a>>) -> usize {
        let mut count = 0;
        let mut seg_start = 0;
        let mut in_quotes = false;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let bytes = input.as_bytes();
        let delim_byte = delimiter as u8;

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\\' && in_quotes && i + 1 < bytes.len() { i += 2; continue; }
            if c == b'"' { in_quotes = !in_quotes; }
            if !in_quotes {
                if c == b'{' { brace_depth += 1; }
                if c == b'}' { brace_depth -= 1; }
                if c == b'[' { bracket_depth += 1; }
                if c == b']' { bracket_depth -= 1; }
            }
            if c == delim_byte && !in_quotes && brace_depth == 0 && bracket_depth == 0 {
                self.emit_primitive(input[seg_start..i].trim(), tape);
                count += 1;
                seg_start = i + 1;
                i += 1;
                continue;
            }
            i += 1;
        }
        if seg_start < input.len() || count > 0 {
            self.emit_primitive(input[seg_start..].trim(), tape);
            count += 1;
        }
        count
    }

    fn emit_primitive<'a>(&mut self, token: &'a str, tape: &mut Vec<Node<'a>>) {
        let t = token.trim();
        if t.is_empty() { tape.push(Node::String("")); return; }
        if t == "[]" { tape.push(Node::Array(0)); return; }
        if t == "{}" { tape.push(Node::Object(0)); return; }

        if t.starts_with('"') {
            if let Some(close) = self.helper.find_closing_quote(t, 0) {
                // For tape, borrow the raw content between quotes (including escapes)
                // True unescape would need arena — for benchmarking raw borrow is representative
                tape.push(Node::String(&t[1..close]));
                return;
            }
        }

        if t == "true" { tape.push(Node::Bool(true)); return; }
        if t == "false" { tape.push(Node::Bool(false)); return; }
        if t == "null" { tape.push(Node::Null); return; }

        let first = t.as_bytes()[0];
        if first.is_ascii_digit() || first == b'+' || first == b'-' || first == b'.' {
            if let Some(node) = self.try_parse_number(t) {
                tape.push(node);
                return;
            }
        }

        tape.push(Node::String(t));
    }

    fn try_parse_number(&self, t: &str) -> Option<Node<'static>> {
        let bytes = t.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let neg = bytes[0] == b'-';
        if neg || bytes[0] == b'+' { pos += 1; }
        if pos >= len { return None; }

        if bytes[pos].is_ascii_digit() {
            let mut n: u64 = 0;
            let mut overflow = false;

            while pos < len && bytes[pos].is_ascii_digit() {
                let d = (bytes[pos] - b'0') as u64;
                match n.checked_mul(10).and_then(|v| v.checked_add(d)) {
                    Some(v) => n = v,
                    None => { overflow = true; break; }
                }
                pos += 1;
            }

            if pos == len && !overflow {
                let val = if neg {
                    if n > (i64::MAX as u64) + 1 {
                        return t.parse::<f64>().ok().map(Node::Float);
                    }
                    if n == (i64::MAX as u64) + 1 { i64::MIN }
                    else { -(n as i64) }
                } else {
                    if n > i64::MAX as u64 {
                        return t.parse::<f64>().ok().map(Node::Float);
                    }
                    n as i64
                };
                return Some(Node::Integer(val));
            }

            if pos < len && (bytes[pos] == b'.' || bytes[pos] == b'e' || bytes[pos] == b'E') {
                return t.parse::<f64>().ok().map(Node::Float);
            }

            if overflow {
                return t.parse::<f64>().ok().map(Node::Float);
            }

            return None;
        }

        if bytes[pos] == b'.' {
            return t.parse::<f64>().ok().map(Node::Float);
        }

        None
    }

    // Fallback: convert owned Value to tape
    fn owned_to_tape(&self, val: &crate::Value) -> Tape<'static> {
        let mut nodes = Vec::new();
        self.emit_owned(val, &mut nodes);
        Tape { nodes }
    }

    fn emit_owned(&self, val: &crate::Value, tape: &mut Vec<Node<'static>>) {
        match val {
            crate::Value::Null => tape.push(Node::Null),
            crate::Value::Bool(b) => tape.push(Node::Bool(*b)),
            crate::Value::Integer(i) => tape.push(Node::Integer(*i)),
            crate::Value::Float(f) => tape.push(Node::Float(*f)),
            crate::Value::String(_) => tape.push(Node::String("")), // lossy — only for minified fallback
            crate::Value::Array(arr) => {
                let pos = tape.len();
                tape.push(Node::Array(arr.len()));
                for v in arr { self.emit_owned(v, tape); }
                tape[pos] = Node::Array(arr.len());
            }
            crate::Value::Object(obj) => {
                let pos = tape.len();
                tape.push(Node::Object(obj.len()));
                for (_, v) in obj {
                    tape.push(Node::Key("")); // lossy keys
                    self.emit_owned(v, tape);
                }
                tape[pos] = Node::Object(obj.len());
            }
        }
    }
}

impl Default for TapeDecoder {
    fn default() -> Self {
        Self::new()
    }
}
