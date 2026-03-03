// scon/src/decoder.rs
// SCON Decoder — SCON string → Value

use crate::value::{Value, SconMap};
use memchr::memchr;

pub struct Decoder {
    indent: usize,
    indent_auto_detect: bool,
    scratch: String, // Scratch buffer compartido — capacidad reutilizada entre llamadas a unescape
}

impl Decoder {
    pub fn new() -> Self {
        Self { indent: 1, indent_auto_detect: true, scratch: String::with_capacity(256) }
    }

    pub fn with_indent(mut self, indent: usize) -> Self {
        self.indent = indent.max(1);
        self.indent_auto_detect = false;
        self
    }

    pub fn decode(&mut self, input: &str) -> Result<Value, String> {
        // P8: Decode minificado directo — evita materializar string expandido (~30% min-decode)
        if self.is_minified(input) {
            return self.decode_minified(input);
        }
        let scon: &str = input;

        //Auto-detect indent
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
                //Scan all lines for first indented non-empty line
                for line in scon.lines() {
                    let spaces = line.len() - line.trim_start_matches(' ').len();
                    if spaces > 0 && !line.trim().is_empty() && !line.trim().starts_with('#') {
                        self.indent = spaces;
                        break;
                    }
                }
            }
        }

        // Pre-alloc estimado — evita re-allocations del Vec durante parsing
        let line_estimate = scon.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1;
        let mut parsed: Vec<ParsedLine<'_>> = Vec::with_capacity(line_estimate);
        let indent = self.indent;
        for (line_num, line) in scon.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let first_byte = trimmed.as_bytes()[0];
            // Skip comments and directives — byte check antes de string compare
            if first_byte == b'#' { continue; }
            if first_byte == b'@' && (trimmed.starts_with("@@") || trimmed.starts_with("@use ")) { continue; }
            if first_byte == b's' && trimmed.starts_with("s:") { continue; }
            if first_byte == b'r' && trimmed.starts_with("r:") { continue; }
            if trimmed.starts_with("sec:") { continue; }
            let spaces = line.len() - line.trim_start_matches(' ').len();
            let depth = if indent > 0 { spaces / indent } else { 0 };
            parsed.push(ParsedLine {
                depth,
                content: trimmed,
                _line_num: line_num,
            });
        }

        if parsed.is_empty() {
            return Ok(Value::Object(SconMap::default()));
        }

        let first = &parsed[0];

        //Empty object
        if parsed.len() == 1 && first.content == "{}" {
            return Ok(Value::Object(SconMap::default()));
        }

        //Array header at root
        if let Some(header) = self.try_array_header(first.content) {
            if header.key.is_none() {
                return self.decode_array_from_header(0, &parsed, &header);
            }
        }

        //Single primitive
        if parsed.len() == 1 && !first.content.contains(':') {
            let val = self.parse_primitive(first.content);
            return Ok(val);
        }

        self.decode_object(0, &parsed, 0).map(|obj| Value::Object(obj))
    }

    // P8: Parsea minified SCON directamente — `;` = newline, `;;` = dedent 1, `;;;` = dedent 2
    // Evita Minifier::expand() que materializa un String completo antes de parsear
    fn decode_minified(&mut self, input: &str) -> Result<Value, String> {
        // Estimar segmentos por conteo de ';'
        let seg_estimate = input.as_bytes().iter().filter(|&&b| b == b';').count() + 1;
        let mut parsed: Vec<ParsedLine<'_>> = Vec::with_capacity(seg_estimate);
        let mut depth: usize = 0;
        let bytes = input.as_bytes();
        let mut seg_start = 0;
        let mut in_quotes = false;
        let mut line_num = 0;

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];

            if c == b'\\' && in_quotes && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_quotes = !in_quotes;
                i += 1;
                continue;
            }

            if c == b';' && !in_quotes {
                let mut semi_count = 1usize;
                while i + 1 < bytes.len() && bytes[i + 1] == b';' {
                    semi_count += 1;
                    i += 1;
                }

                let segment = input[seg_start..i - semi_count + 1].trim();
                if !segment.is_empty() && !segment.starts_with('#') {
                    if !(segment.starts_with("@@") || segment.starts_with("s:") || segment.starts_with("r:") || segment.starts_with("sec:") || segment.starts_with("@use ")) {
                        parsed.push(ParsedLine { depth, content: segment, _line_num: line_num });
                        line_num += 1;

                        // Scope openers: key: → depth+1
                        if segment.ends_with(':') && !self.has_inline_value_after_colon(segment) {
                            depth += 1;
                        }
                        // List items: "- " → depth+1
                        if segment.starts_with("- ") {
                            depth += 1;
                        }
                    }
                }

                // Dedent por semicolons múltiples
                if semi_count >= 2 {
                    depth = depth.saturating_sub(semi_count - 1);
                }

                seg_start = i + 1;
                i += 1;
                continue;
            }

            i += 1;
        }

        // Último segmento
        let segment = input[seg_start..].trim();
        if !segment.is_empty() && !segment.starts_with('#') {
            if !(segment.starts_with("@@") || segment.starts_with("s:") || segment.starts_with("r:") || segment.starts_with("sec:") || segment.starts_with("@use ")) {
                parsed.push(ParsedLine { depth, content: segment, _line_num: line_num });
            }
        }

        if parsed.is_empty() {
            return Ok(Value::Object(SconMap::default()));
        }

        let first = &parsed[0];
        if parsed.len() == 1 && first.content == "{}" {
            return Ok(Value::Object(SconMap::default()));
        }
        if let Some(header) = self.try_array_header(first.content) {
            if header.key.is_none() {
                return self.decode_array_from_header(0, &parsed, &header);
            }
        }
        if parsed.len() == 1 && !first.content.contains(':') {
            return Ok(self.parse_primitive(first.content));
        }

        self.decode_object(0, &parsed, 0).map(|obj| Value::Object(obj))
    }

    // Helper para decode_minified: determina si una línea tiene valor inline después del colon
    fn has_inline_value_after_colon(&self, s: &str) -> bool {
        if let Some(colon_pos) = self.find_key_colon(s) {
            let after = s[colon_pos + 1..].trim();
            !after.is_empty()
        } else {
            false
        }
    }

    fn is_minified(&self, s: &str) -> bool {
        !s.contains('\n') && s.contains(';')
    }

    // --- Object decoding ---

    fn decode_object(&mut self, base_depth: usize, lines: &[ParsedLine<'_>], start: usize) -> Result<SconMap<String, Value>, String> {
        let mut result = SconMap::default();
        let mut i = start;

        while i < lines.len() {
            let line = &lines[i];
            if line.depth < base_depth { break; }
            if line.depth > base_depth { i += 1; continue; }

            let content = line.content;

            //P1.4: Try parse_array_header directly — single pass instead of is+parse
            if let Some(header) = self.try_array_header(content) {
                if let Some(key) = header.key {
                    let val = self.decode_array_from_header(i, lines, &header)?;
                    result.insert(key.to_string(), val);
                    i += 1;
                    while i < lines.len() && lines[i].depth > base_depth { i += 1; }
                    continue;
                }
            }

            //Key-value
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

    // P5: colon_pos ya viene de find_key_colon — evita re-escanear en parse_key
    fn decode_key_value(&mut self, line: &ParsedLine<'_>, lines: &[ParsedLine<'_>], index: usize, base_depth: usize, colon_pos: usize) -> Result<(String, Value, usize), String> {
        let content = line.content;
        let (key, key_end) = if content.as_bytes()[0] == b'"' {
            self.parse_key(content)?
        } else {
            // Fast path: colon_pos ya conocido, no re-escanear
            (content[..colon_pos].trim().to_string(), colon_pos + 1)
        };
        let rest = content[key_end..].trim();

        if !rest.is_empty() {
            //Inline value: could be {}, [], or primitive
            let value = self.parse_inline_value(rest);
            return Ok((key, value, index + 1));
        }

        //Nested object/value
        if index + 1 < lines.len() && lines[index + 1].depth > base_depth {
            //Check if children are list items → array
            let child_depth = base_depth + 1;
            if index + 1 < lines.len() && lines[index + 1].depth == child_depth && lines[index + 1].content.starts_with("- ") {
                //It's an expanded array without header
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

        //Empty nested (key: with nothing after)
        Ok((key, Value::Object(SconMap::default()), index + 1))
    }

    // --- Array decoding ---

    fn decode_array_from_header(&mut self, index: usize, lines: &[ParsedLine<'_>], header: &ArrayHeader<'_>) -> Result<Value, String> {
        if header.length == 0 {
            return Ok(Value::Array(vec![]));
        }

        //Inline values
        if let Some(ref inline) = header.inline_values {
            if header.fields.is_none() {
                let values = self.parse_delimited_values(inline, header.delimiter);
                return Ok(Value::Array(values));
            }
        }

        //Tabular
        if let Some(ref fields) = header.fields {
            return self.decode_tabular_array(index, lines, header.length, fields, header.delimiter);
        }

        //Expanded
        self.decode_expanded_array(index, lines, header.length)
    }

    fn decode_tabular_array(&mut self, header_idx: usize, lines: &[ParsedLine<'_>], expected: usize, fields: &[&str], delimiter: char) -> Result<Value, String> {
        let base_depth = lines[header_idx].depth;
        let mut result = Vec::with_capacity(expected);
        let mut i = header_idx + 1;

        while i < lines.len() && result.len() < expected {
            if lines[i].depth != base_depth + 1 { break; }
            let mut values = self.parse_delimited_values(lines[i].content, delimiter);
            // Pre-pad con Null si faltan campos
            values.resize(fields.len(), Value::Null);
            let mut row = SconMap::with_capacity_and_hasher(fields.len(), ahash::RandomState::new());
            // Consumir values sin clone — drain evita copia de cada Value
            for (field, val) in fields.iter().zip(values.drain(..)) {
                row.insert(field.to_string(), val);
            }
            result.push(Value::Object(row));
            i += 1;
        }

        Ok(Value::Array(result))
    }

    fn decode_expanded_array(&mut self, header_idx: usize, lines: &[ParsedLine<'_>], expected: usize) -> Result<Value, String> {
        let base_depth = lines[header_idx].depth;
        let mut result = Vec::with_capacity(expected);
        let mut i = header_idx + 1;

        while i < lines.len() && result.len() < expected {
            let line = &lines[i];
            if line.depth != base_depth + 1 { break; }

            if line.content.starts_with("- ") {
                let item_content = &line.content[2..];

                if let Some(header) = self.try_array_header(item_content) {
                    if header.key.is_some() {
                        //Array-valued first field of an object (e.g., "- deps[2]: a, b")
                        let obj = self.decode_list_item_object(line, lines, i, base_depth)?;
                        result.push(Value::Object(obj));
                        i += 1;
                        while i < lines.len() && lines[i].depth > base_depth + 1 { i += 1; }
                        continue;
                    } else if let Some(ref inline) = header.inline_values {
                        //Bare inline array item (e.g., "- [3]: a, b, c")
                        result.push(Value::Array(self.parse_delimited_values(inline, header.delimiter)));
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

    fn decode_list_item_object(&mut self, _line: &ParsedLine<'_>, lines: &[ParsedLine<'_>], index: usize, base_depth: usize) -> Result<SconMap<String, Value>, String> {
        let item_content = &lines[index].content[2..]; // skip "- "

        let mut result = SconMap::default();
        let cont_depth = base_depth + 2;

        //First field may be an array header (e.g., "dependencies[1]: auth")
        if let Some(header) = self.try_array_header(item_content) {
            if let Some(key) = header.key {
                let val = self.decode_array_from_header(index, lines, &header)?;
                result.insert(key.to_string(), val);
            }
        } else {
            let (key, key_end) = self.parse_key(item_content)?;
            let rest = item_content[key_end..].trim();

            if !rest.is_empty() {
                result.insert(key, self.parse_inline_value(rest));
            } else if index + 1 < lines.len() && lines[index + 1].depth >= cont_depth {
                let obj = self.decode_object(cont_depth, lines, index + 1)?;
                result.insert(key, Value::Object(obj));
            } else {
                result.insert(key, Value::Object(SconMap::default()));
            }
        }

        //Continuation fields
        let mut i = index + 1;
        while i < lines.len() {
            let next = &lines[i];
            if next.depth < cont_depth { break; }
            if next.depth == cont_depth {
                if next.content.starts_with("- ") { break; }

                //Array header in continuation
                if let Some(header) = self.try_array_header(next.content) {
                    if let Some(k) = header.key {
                        let val = self.decode_array_from_header(i, lines, &header)?;
                        result.insert(k.to_string(), val);
                        i += 1;
                        while i < lines.len() && lines[i].depth > cont_depth { i += 1; }
                        continue;
                    }
                }

                if let Some(colon_pos) = self.find_key_colon(next.content) {
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

    //P1.4: Single-pass check+parse — replaces is_array_header + parse_array_header
    fn try_array_header<'a>(&self, content: &'a str) -> Option<ArrayHeader<'a>> {
        let bracket_start = content.find('[')?;
        //Quick reject: bracket must appear before first colon
        let colon_pos = content.find(':')?;
        if bracket_start >= colon_pos { return None; }

        self.parse_array_header(content)
    }

    // Zero-alloc: retorna slices prestados del content original
    fn parse_array_header<'a>(&self, content: &'a str) -> Option<ArrayHeader<'a>> {
        let bracket_start = content.find('[')?;
        let key = if bracket_start > 0 {
            let raw = content[..bracket_start].trim();
            // Fast path: la mayoría de keys no tienen quotes
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

        //Fields {field1,field2} — split_top_level ya retorna &str slices
        let mut fields = None;
        let after_bracket = &content[bracket_end + 1..];
        if after_bracket.starts_with('{') {
            if let Some(brace_end) = after_bracket.find('}') {
                let fields_str = &after_bracket[1..brace_end];
                let parts = self.split_top_level(fields_str, delimiter);
                fields = Some(parts.into_iter().map(|s| s.trim()).collect());
            }
        }

        //Inline values after : — slice del content, no copia
        let colon_pos = content.rfind(':')?;
        let after_colon = content[colon_pos + 1..].trim();
        let inline_values = if !after_colon.is_empty() {
            Some(after_colon)
        } else {
            None
        };

        Some(ArrayHeader { key, length, delimiter, fields, inline_values })
    }

    fn parse_key(&mut self, content: &str) -> Result<(String, usize), String> {
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

    // P7: memchr SIMD fast-path — threshold 20 bytes (bajo ese largo, scan manual es más rápido)
    fn find_key_colon(&self, s: &str) -> Option<usize> {
        let bytes = s.as_bytes();

        if bytes.len() >= 20 {
            // SIMD path: buscar ':'. Si no hay '"' ni '{' antes, es el colon del key
            if let Some(colon) = memchr(b':', bytes) {
                let prefix = &bytes[..colon];
                if !prefix.contains(&b'"') && !prefix.contains(&b'{') {
                    return Some(colon);
                }
            } else {
                return None;
            }
        }

        // Scan manual para strings cortos o con quotes/braces
        let mut in_quotes = false;
        let mut brace_depth = 0i32;
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

    fn parse_inline_value(&mut self, input: &str) -> Value {
        let trimmed = input.trim();
        if trimmed.is_empty() { return Value::String(String::new()); }
        if trimmed == "[]" { return Value::Array(vec![]); }
        if trimmed == "{}" { return Value::Object(SconMap::default()); }

        //Inline object {key:val, ...}
        if trimmed.starts_with('{') {
            if let Some(inner) = self.extract_brace_content(trimmed) {
                return Value::Object(self.parse_inline_object(inner));
            }
        }

        //Inline array [a, b, c]
        if trimmed.starts_with('[') {
            if let Some(close) = self.find_matching_bracket(trimmed, 0) {
                let inner = &trimmed[1..close];
                let items = self.parse_delimited_values(inner, ',');
                return Value::Array(items);
            }
        }

        self.parse_primitive(trimmed)
    }

    fn parse_inline_object(&mut self, inner: &str) -> SconMap<String, Value> {
        let mut result = SconMap::default();
        let parts = self.split_top_level(inner, ',');

        for part in &parts {
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

    fn parse_delimited_values(&mut self, input: &str, delimiter: char) -> Vec<Value> {
        let parts = self.split_top_level(input, delimiter);
        parts.iter().map(|p| self.parse_primitive(p.trim())).collect()
    }

    fn parse_primitive(&mut self, token: &str) -> Value {
        let t = token.trim();
        if t.is_empty() { return Value::String(String::new()); }
        if t == "[]" { return Value::Array(vec![]); }
        if t == "{}" { return Value::Object(SconMap::default()); }

        if t.starts_with('"') {
            if let Some(close) = self.find_closing_quote(t, 0) {
                return Value::String(self.unescape_string(&t[1..close]));
            }
        }

        if t == "true" { return Value::Bool(true); }
        if t == "false" { return Value::Bool(false); }
        if t == "null" { return Value::Null; }

        // Parser numérico manual — acumulador byte-level para enteros, stdlib solo para floats
        // Evita overhead de parse::<i64>() (error allocation, Result branching)
        let first = t.as_bytes()[0];
        if first.is_ascii_digit() || first == b'+' || first == b'-' || first == b'.' {
            if let Some(val) = self.try_parse_number(t) {
                return val;
            }
        }

        Value::String(t.to_string())
    }

    // Parser numérico manual — enteros por acumulador byte-level, floats por stdlib (pre-validado)
    // Enteros son el caso común: evita FromStr + Result + ParseIntError allocation
    // Floats: validamos formato primero → stdlib parse no falla → no paga error path
    fn try_parse_number(&self, t: &str) -> Option<Value> {
        let bytes = t.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let neg = bytes[0] == b'-';
        if neg || bytes[0] == b'+' { pos += 1; }
        if pos >= len { return None; }

        // Empieza con dígito → integer o float
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

            // Entero puro: todos los bytes consumidos, sin overflow
            if pos == len && !overflow {
                let val = if neg {
                    if n > (i64::MAX as u64) + 1 {
                        return t.parse::<f64>().ok().map(Value::Float);
                    }
                    if n == (i64::MAX as u64) + 1 { i64::MIN }
                    else { -(n as i64) }
                } else {
                    if n > i64::MAX as u64 {
                        return t.parse::<f64>().ok().map(Value::Float);
                    }
                    n as i64
                };
                return Some(Value::Integer(val));
            }

            // Punto decimal o exponente → float (stdlib, formato ya validado)
            if pos < len && (bytes[pos] == b'.' || bytes[pos] == b'e' || bytes[pos] == b'E') {
                return t.parse::<f64>().ok().map(Value::Float);
            }

            // Overflow sin punto/exp → float
            if overflow {
                return t.parse::<f64>().ok().map(Value::Float);
            }

            return None;
        }

        // Empieza con '.' → float
        if bytes[pos] == b'.' {
            return t.parse::<f64>().ok().map(Value::Float);
        }

        None
    }

    fn unquote_key(&mut self, s: &str) -> String {
        let t = s.trim();
        if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
            self.unescape_string(&t[1..t.len() - 1])
        } else {
            t.to_string()
        }
    }

    // P7: memchr2 SIMD — salta segmentos sin escapes ni quotes
    fn find_closing_quote(&self, s: &str, start: usize) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut i = start + 1;
        while i < bytes.len() {
            match memchr::memchr2(b'\\', b'"', &bytes[i..]) {
                Some(offset) => {
                    let pos = i + offset;
                    if bytes[pos] == b'"' { return Some(pos); }
                    // Es backslash — saltar el escape
                    i = pos + 2;
                }
                None => return None,
            }
        }
        None
    }

    // Fast-path + scratch buffer — dos optimizaciones combinadas:
    // 1. Si no hay backslashes, retorna slice directo sin procesar (mayoría de strings)
    // 2. Si hay escapes, usa scratch buffer compartido (capacidad reutilizada entre llamadas)
    fn unescape_string(&mut self, s: &str) -> String {
        // Fast path: sin backslashes → copia directa, skip byte-by-byte loop
        if memchr(b'\\', s.as_bytes()).is_none() {
            return s.to_string();
        }
        // Slow path: scratch buffer — clear reutiliza capacidad previa
        self.scratch.clear();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'\\' => self.scratch.push('\\'),
                    b'"' => self.scratch.push('"'),
                    b'n' => self.scratch.push('\n'),
                    b'r' => self.scratch.push('\r'),
                    b't' => self.scratch.push('\t'),
                    b';' => self.scratch.push(';'),
                    other => {
                        self.scratch.push('\\');
                        self.scratch.push(other as char);
                    }
                }
                i += 2;
            } else {
                self.scratch.push(bytes[i] as char);
                i += 1;
            }
        }
        self.scratch.clone()
    }

    //P1.3: Returns &str slice instead of String — zero-copy
    fn extract_brace_content<'a>(&self, input: &'a str) -> Option<&'a str> {
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
                        return Some(&input[s + 1..i]);
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

    //P1.2: Returns Vec<&str> slices instead of Vec<String> — zero-copy
    fn split_top_level<'a>(&self, input: &'a str, delimiter: char) -> Vec<&'a str> {
        let mut parts = Vec::new();
        let mut seg_start = 0;
        let mut in_quotes = false;
        let mut brace_depth = 0i32;
        let mut bracket_depth = 0i32;
        let bytes = input.as_bytes();
        let delim_byte = delimiter as u8;

        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\\' && in_quotes && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' { in_quotes = !in_quotes; }
            if !in_quotes {
                if c == b'{' { brace_depth += 1; }
                if c == b'}' { brace_depth -= 1; }
                if c == b'[' { bracket_depth += 1; }
                if c == b']' { bracket_depth -= 1; }
            }
            if c == delim_byte && !in_quotes && brace_depth == 0 && bracket_depth == 0 {
                parts.push(&input[seg_start..i]);
                seg_start = i + 1;
                i += 1;
                continue;
            }
            i += 1;
        }
        if seg_start < input.len() || !parts.is_empty() {
            parts.push(&input[seg_start..]);
        }
        parts
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

//P1.1: Zero-copy — content borrows from input string
struct ParsedLine<'a> {
    depth: usize,
    content: &'a str,
    _line_num: usize,
}

// Zero-alloc: todos los campos son slices prestados del input original
struct ArrayHeader<'a> {
    key: Option<&'a str>,
    length: usize,
    delimiter: char,
    fields: Option<Vec<&'a str>>,
    inline_values: Option<&'a str>,
}
