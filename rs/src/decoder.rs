// scon/src/decoder.rs
// SCON Decoder — SCON string → Value

use crate::minifier::Minifier;
use crate::value::Value;
use indexmap::IndexMap;

pub struct Decoder {
    indent: usize,
    indent_auto_detect: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self { indent: 1, indent_auto_detect: true }
    }

    pub fn with_indent(mut self, indent: usize) -> Self {
        self.indent = indent.max(1);
        self.indent_auto_detect = false;
        self
    }

    pub fn decode(&mut self, input: &str) -> Result<Value, String> {
        // Avoid unconditional clone - only allocate if minified input needs expansion
        let expanded;
        let scon: &str = if self.is_minified(input) {
            expanded = Minifier::expand(input, self.indent);
            &expanded
        } else {
            input
        };

        // Auto-detect indent
        if self.indent_auto_detect {
            if let Some(cap) = scon.find('\n').and_then(|nl_pos| {
                let after = &scon[nl_pos + 1..];
                let spaces = after.len() - after.trim_start_matches(' ').len();
                if spaces > 0 && after.len() > spaces && !after.as_bytes().get(spaces).map_or(true, |b| b.is_ascii_whitespace()) {
                    Some(spaces)
                } else {
                    None
                }
            }) {
                self.indent = cap;
            } else {
                // Scan all lines for first indented non-empty line
                for line in scon.lines() {
                    let spaces = line.len() - line.trim_start_matches(' ').len();
                    if spaces > 0 && !line.trim().is_empty() && !line.trim().starts_with('#') {
                        self.indent = spaces;
                        break;
                    }
                }
            }
        }

        // Parse lines
        let mut parsed: Vec<ParsedLine> = Vec::new();
        for (line_num, line) in scon.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Skip directives and schema defs (not implemented yet)
            if trimmed.starts_with("@@") || trimmed.starts_with("s:") || trimmed.starts_with("r:") || trimmed.starts_with("sec:") || trimmed.starts_with("@use ") {
                continue;
            }
            let depth = self.calculate_depth(line);
            parsed.push(ParsedLine {
                depth,
                content: trimmed.to_string(),
                _line_num: line_num,
            });
        }

        if parsed.is_empty() {
            return Ok(Value::Object(IndexMap::new()));
        }

        let first = &parsed[0];

        // Empty object
        if parsed.len() == 1 && first.content == "{}" {
            return Ok(Value::Object(IndexMap::new()));
        }

        // Array header at root
        if self.is_array_header(&first.content) {
            if let Some(header) = self.parse_array_header(&first.content) {
                if header.key.is_none() {
                    return self.decode_array_from_header(0, &parsed, &header);
                }
            }
        }

        // Single primitive
        if parsed.len() == 1 && !first.content.contains(':') {
            let val = self.parse_primitive(&first.content);
            return Ok(val);
        }

        self.decode_object(0, &parsed, 0).map(|obj| Value::Object(obj))
    }

    fn calculate_depth(&self, line: &str) -> usize {
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if self.indent > 0 { spaces / self.indent } else { 0 }
    }

    fn is_minified(&self, s: &str) -> bool {
        !s.contains('\n') && s.contains(';')
    }

    // --- Object decoding ---

    fn decode_object(&self, base_depth: usize, lines: &[ParsedLine], start: usize) -> Result<IndexMap<String, Value>, String> {
        let mut result = IndexMap::new();
        let mut i = start;

        while i < lines.len() {
            let line = &lines[i];
            if line.depth < base_depth { break; }
            if line.depth > base_depth { i += 1; continue; }

            let content = &line.content;

            // Array header with key
            if self.is_array_header(content) {
                if let Some(header) = self.parse_array_header(content) {
                    if let Some(ref key) = header.key {
                        let val = self.decode_array_from_header(i, lines, &header)?;
                        result.insert(key.clone(), val);
                        i += 1;
                        while i < lines.len() && lines[i].depth > base_depth { i += 1; }
                        continue;
                    }
                }
            }

            // Key-value
            if let Some(colon_pos) = self.find_key_colon(content) {
                let (key, value, next_i) = self.decode_key_value(line, lines, i, base_depth, colon_pos)?;
                result.insert(key, value);
                i = next_i;
                continue;
            }

            i += 1;
        }

        Ok(result)
    }

    fn decode_key_value(&self, line: &ParsedLine, lines: &[ParsedLine], index: usize, base_depth: usize, _colon_pos: usize) -> Result<(String, Value, usize), String> {
        let content = &line.content;
        let (key, key_end) = self.parse_key(content)?;
        let rest = content[key_end..].trim();

        if !rest.is_empty() {
            // Inline value: could be {}, [], or primitive
            let value = self.parse_inline_value(rest);
            return Ok((key, value, index + 1));
        }

        // Nested object/value
        if index + 1 < lines.len() && lines[index + 1].depth > base_depth {
            // Check if children are list items → array
            let child_depth = base_depth + 1;
            if index + 1 < lines.len() && lines[index + 1].depth == child_depth && lines[index + 1].content.starts_with("- ") {
                // It's an expanded array without header
                let mut items = Vec::new();
                let mut j = index + 1;
                while j < lines.len() && lines[j].depth >= child_depth {
                    if lines[j].depth == child_depth && lines[j].content.starts_with("- ") {
                        let item_content = &lines[j].content[2..];
                        if item_content.contains(':') {
                            let obj = self.decode_list_item_object(&lines[j], lines, j, base_depth)?;
                            items.push(Value::Object(obj));
                        } else {
                            items.push(self.parse_inline_value(item_content));
                        }
                    }
                    j += 1;
                }
                return Ok((key, Value::Array(items), j));
            }

            let obj = self.decode_object(base_depth + 1, lines, index + 1)?;
            let mut next_i = index + 1;
            while next_i < lines.len() && lines[next_i].depth > base_depth { next_i += 1; }
            return Ok((key, Value::Object(obj), next_i));
        }

        // Empty nested (key: with nothing after)
        Ok((key, Value::Object(IndexMap::new()), index + 1))
    }

    // --- Array decoding ---

    fn decode_array_from_header(&self, index: usize, lines: &[ParsedLine], header: &ArrayHeader) -> Result<Value, String> {
        if header.length == 0 {
            return Ok(Value::Array(vec![]));
        }

        // Inline values
        if let Some(ref inline) = header.inline_values {
            if header.fields.is_none() {
                let values = self.parse_delimited_values(inline, header.delimiter);
                return Ok(Value::Array(values));
            }
        }

        // Tabular
        if let Some(ref fields) = header.fields {
            return self.decode_tabular_array(index, lines, header.length, fields, header.delimiter);
        }

        // Expanded
        self.decode_expanded_array(index, lines, header.length)
    }

    fn decode_tabular_array(&self, header_idx: usize, lines: &[ParsedLine], expected: usize, fields: &[String], delimiter: char) -> Result<Value, String> {
        let base_depth = lines[header_idx].depth;
        let mut result = Vec::with_capacity(expected);
        let mut i = header_idx + 1;

        while i < lines.len() && result.len() < expected {
            if lines[i].depth != base_depth + 1 { break; }
            let values = self.parse_delimited_values(&lines[i].content, delimiter);
            let mut row = IndexMap::new();
            for (j, field) in fields.iter().enumerate() {
                row.insert(field.clone(), values.get(j).cloned().unwrap_or(Value::Null));
            }
            result.push(Value::Object(row));
            i += 1;
        }

        Ok(Value::Array(result))
    }

    fn decode_expanded_array(&self, header_idx: usize, lines: &[ParsedLine], expected: usize) -> Result<Value, String> {
        let base_depth = lines[header_idx].depth;
        let mut result = Vec::with_capacity(expected);
        let mut i = header_idx + 1;

        while i < lines.len() && result.len() < expected {
            let line = &lines[i];
            if line.depth != base_depth + 1 { break; }

            if line.content.starts_with("- ") {
                let item_content = &line.content[2..];

                if self.is_array_header(item_content) {
                    if let Some(header) = self.parse_array_header(item_content) {
                        if let Some(ref inline) = header.inline_values {
                            result.push(Value::Array(self.parse_delimited_values(inline, header.delimiter)));
                        }
                    }
                } else if item_content.contains(':') {
                    let obj = self.decode_list_item_object(line, lines, i, base_depth)?;
                    result.push(Value::Object(obj));
                    i += 1;
                    while i < lines.len() && lines[i].depth > base_depth + 1 { i += 1; }
                    continue;
                } else {
                    result.push(self.parse_inline_value(item_content));
                }
            }
            i += 1;
        }

        Ok(Value::Array(result))
    }

    fn decode_list_item_object(&self, _line: &ParsedLine, lines: &[ParsedLine], index: usize, base_depth: usize) -> Result<IndexMap<String, Value>, String> {
        let item_content = &lines[index].content[2..]; // skip "- "
        let (key, key_end) = self.parse_key(item_content)?;
        let rest = item_content[key_end..].trim();

        let mut result = IndexMap::new();
        let cont_depth = base_depth + 2;

        if !rest.is_empty() {
            result.insert(key, self.parse_inline_value(rest));
        } else if index + 1 < lines.len() && lines[index + 1].depth >= cont_depth {
            let obj = self.decode_object(cont_depth, lines, index + 1)?;
            result.insert(key, Value::Object(obj));
        } else {
            result.insert(key, Value::Object(IndexMap::new()));
        }

        // Continuation fields
        let mut i = index + 1;
        while i < lines.len() {
            let next = &lines[i];
            if next.depth < cont_depth { break; }
            if next.depth == cont_depth {
                if next.content.starts_with("- ") { break; }

                // Array header in continuation
                if self.is_array_header(&next.content) {
                    if let Some(header) = self.parse_array_header(&next.content) {
                        if let Some(ref k) = header.key {
                            let val = self.decode_array_from_header(i, lines, &header)?;
                            result.insert(k.clone(), val);
                            i += 1;
                            while i < lines.len() && lines[i].depth > cont_depth { i += 1; }
                            continue;
                        }
                    }
                }

                if let Some(colon_pos) = self.find_key_colon(&next.content) {
                    let (k, v, next_i) = self.decode_key_value(next, lines, i, cont_depth, colon_pos)?;
                    result.insert(k, v);
                    i = next_i;
                    continue;
                }
            }
            i += 1;
        }

        Ok(result)
    }

    // --- Parsing helpers ---

    fn parse_array_header(&self, content: &str) -> Option<ArrayHeader> {
        let bracket_start = content.find('[')?;
        let key = if bracket_start > 0 {
            let raw = content[..bracket_start].trim();
            Some(self.unquote_key(raw))
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

        // Fields {field1,field2}
        let mut fields = None;
        let after_bracket = &content[bracket_end + 1..];
        if after_bracket.starts_with('{') {
            if let Some(brace_end) = after_bracket.find('}') {
                let fields_str = &after_bracket[1..brace_end];
                fields = Some(self.parse_delimited_values(fields_str, delimiter)
                    .into_iter()
                    .map(|v| match v {
                        Value::String(s) => s,
                        _ => v.to_string(),
                    })
                    .collect());
            }
        }

        // Inline values after :
        let colon_pos = content.rfind(':')?;
        let after_colon = content[colon_pos + 1..].trim();
        let inline_values = if !after_colon.is_empty() {
            Some(after_colon.to_string())
        } else {
            None
        };

        Some(ArrayHeader { key, length, delimiter, fields, inline_values })
    }

    fn is_array_header(&self, content: &str) -> bool {
        let bracket_pos = content.find('[');
        let colon_pos = content.find(':');
        match (bracket_pos, colon_pos) {
            (Some(b), Some(c)) => b < c,
            _ => false,
        }
    }

    fn parse_key(&self, content: &str) -> Result<(String, usize), String> {
        if content.starts_with('"') {
            let close = self.find_closing_quote(content, 0)
                .ok_or_else(|| "Unterminated quoted key".to_string())?;
            let key = self.unescape_string(&content[1..close]);
            if close + 1 >= content.len() || content.as_bytes()[close + 1] != b':' {
                return Err("Missing colon after key".to_string());
            }
            Ok((key, close + 2))
        } else {
            let colon = content.find(':').ok_or_else(|| "Missing colon after key".to_string())?;
            let key = content[..colon].trim().to_string();
            Ok((key, colon + 1))
        }
    }

    fn find_key_colon(&self, s: &str) -> Option<usize> {
        let mut in_quotes = false;
        let mut brace_depth = 0i32;
        let bytes = s.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\\' && in_quotes && i + 1 < bytes.len() { i += 2; continue; }
            if c == b'"' { in_quotes = !in_quotes; i += 1; continue; }
            if !in_quotes {
                if c == b'{' { brace_depth += 1; }
                if c == b'}' { brace_depth -= 1; }
                if c == b':' && brace_depth == 0 { return Some(i); }
            }
            i += 1;
        }
        None
    }

    fn parse_inline_value(&self, input: &str) -> Value {
        let trimmed = input.trim();
        if trimmed.is_empty() { return Value::String(String::new()); }
        if trimmed == "[]" { return Value::Array(vec![]); }
        if trimmed == "{}" { return Value::Object(IndexMap::new()); }

        // Inline object {key:val, ...}
        if trimmed.starts_with('{') {
            if let Some(inner) = self.extract_brace_content(trimmed) {
                return Value::Object(self.parse_inline_object(&inner));
            }
        }

        // Inline array [a, b, c]
        if trimmed.starts_with('[') {
            if let Some(close) = self.find_matching_bracket(trimmed, 0) {
                let inner = &trimmed[1..close];
                let items = self.parse_delimited_values(inner, ',');
                return Value::Array(items);
            }
        }

        self.parse_primitive(trimmed)
    }

    fn parse_inline_object(&self, inner: &str) -> IndexMap<String, Value> {
        let mut result = IndexMap::new();
        let parts = self.split_top_level(inner, ',');

        for part in parts {
            let part = part.trim();
            if part.is_empty() { continue; }
            if let Some(colon) = self.find_key_colon(part) {
                let key = self.unquote_key(part[..colon].trim());
                let val = self.parse_inline_value(part[colon + 1..].trim());
                result.insert(key, val);
            }
        }

        result
    }

    fn parse_delimited_values(&self, input: &str, delimiter: char) -> Vec<Value> {
        let parts = self.split_top_level(input, delimiter);
        parts.iter().map(|p| self.parse_primitive(p.trim())).collect()
    }

    fn parse_primitive(&self, token: &str) -> Value {
        let t = token.trim();
        if t.is_empty() { return Value::String(String::new()); }
        if t == "[]" { return Value::Array(vec![]); }
        if t == "{}" { return Value::Object(IndexMap::new()); }

        if t.starts_with('"') {
            if let Some(close) = self.find_closing_quote(t, 0) {
                return Value::String(self.unescape_string(&t[1..close]));
            }
        }

        if t == "true" { return Value::Bool(true); }
        if t == "false" { return Value::Bool(false); }
        if t == "null" { return Value::Null; }

        // Integer
        if let Ok(n) = t.parse::<i64>() {
            return Value::Integer(n);
        }

        // Float
        if let Ok(n) = t.parse::<f64>() {
            return Value::Float(n);
        }

        Value::String(t.to_string())
    }

    fn unquote_key(&self, s: &str) -> String {
        let t = s.trim();
        if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
            self.unescape_string(&t[1..t.len() - 1])
        } else {
            t.to_string()
        }
    }

    fn find_closing_quote(&self, s: &str, start: usize) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut i = start + 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() { i += 2; continue; }
            if bytes[i] == b'"' { return Some(i); }
            i += 1;
        }
        None
    }

    fn unescape_string(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'\\' => result.push('\\'),
                    b'"' => result.push('"'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    b';' => result.push(';'),
                    other => {
                        result.push('\\');
                        result.push(other as char);
                    }
                }
                i += 2;
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
        result
    }

    fn extract_brace_content(&self, input: &str) -> Option<String> {
        let mut depth = 0i32;
        let mut start = None;
        let mut in_quotes = false;
        let bytes = input.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\\' && in_quotes && i + 1 < bytes.len() { i += 2; continue; }
            if c == b'"' { in_quotes = !in_quotes; i += 1; continue; }
            if !in_quotes {
                if c == b'{' {
                    if depth == 0 { start = Some(i); }
                    depth += 1;
                }
                if c == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        let s = start.unwrap_or(0);
                        return Some(input[s + 1..i].to_string());
                    }
                }
            }
            i += 1;
        }
        None
    }

    fn find_matching_bracket(&self, s: &str, start: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut in_quotes = false;
        let bytes = s.as_bytes();

        let mut i = start;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\\' && in_quotes && i + 1 < bytes.len() { i += 2; continue; }
            if c == b'"' { in_quotes = !in_quotes; i += 1; continue; }
            if !in_quotes {
                if c == b'[' { depth += 1; }
                if c == b']' {
                    depth -= 1;
                    if depth == 0 { return Some(i); }
                }
            }
            i += 1;
        }
        None
    }

    fn split_top_level(&self, input: &str, delimiter: char) -> Vec<String> {
        let mut parts = Vec::new();
        let mut buffer = String::new();
        let mut in_quotes = false;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let bytes = input.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if bytes[i] == b'\\' && in_quotes && i + 1 < bytes.len() {
                buffer.push(c);
                buffer.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == '"' { in_quotes = !in_quotes; }
            if !in_quotes {
                if c == '{' { brace_depth += 1; }
                if c == '}' { brace_depth -= 1; }
                if c == '[' { bracket_depth += 1; }
                if c == ']' { bracket_depth -= 1; }
            }
            if c == delimiter && !in_quotes && brace_depth == 0 && bracket_depth == 0 {
                parts.push(std::mem::take(&mut buffer));
                i += 1;
                continue;
            }
            buffer.push(c);
            i += 1;
        }
        if !buffer.is_empty() || !parts.is_empty() {
            parts.push(buffer);
        }
        parts
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

struct ParsedLine {
    depth: usize,
    content: String,
    _line_num: usize,
}

struct ArrayHeader {
    key: Option<String>,
    length: usize,
    delimiter: char,
    fields: Option<Vec<String>>,
    inline_values: Option<String>,
}
