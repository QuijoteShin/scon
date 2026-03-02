// js/decoder.js
// Port of bX\Scon\Decoder — SCON string → JS object

import { SchemaRegistry } from './schema-registry.js';
import { Minifier } from './minifier.js';

const COMMA = ',';
const TAB = '\t';
const PIPE = '|';
const DQUOTE = '"';
const COLON = ':';
const OBRACK = '[';
const CBRACK = ']';
const OBRACE = '{';
const CBRACE = '}';
const LIST_PREFIX = '- ';
const BACKSLASH = '\\';
const SEMICOLON = ';';

export class Decoder {

    constructor(options = {}) {
        // Si indent no se provee, auto-detectar del documento
        this._indentAutoDetect = !('indent' in options);
        this.indent = options.indent ?? 1;
        this.strict = options.strict ?? true;
        this.registry = new SchemaRegistry();
        this.directives = {};
    }

    getRegistry() { return this.registry; }
    getDirectives() { return this.directives; }

    // Decode SCON string to JS object/array
    decode(sconString) {
        // Expand if minified
        if (this._isMinified(sconString)) {
            sconString = Minifier.expand(sconString, this.indent);
        }

        // Auto-detect indent: primera línea que empiece con espacios
        if (this._indentAutoDetect) {
            const m = sconString.match(/\n( +)\S/);
            if (m) this.indent = m[1].length;
            this._indentAutoDetect = false;
        }

        const lines = sconString.split('\n');
        const parsedLines = [];

        // First pass: extract header, directives, and definitions
        for (let lineNum = 0; lineNum < lines.length; lineNum++) {
            const line = lines[lineNum];
            const trimmed = line.trim();

            // Skip empty lines and comments
            if (trimmed === '' || trimmed[0] === '#') {
                continue;
            }

            // Directives (@@)
            if (trimmed.startsWith('@@')) {
                this._parseDirective(trimmed);
                continue;
            }

            // Schema definition (s:name ...)
            let match = trimmed.match(/^s:(\S+)\s+/);
            if (match) {
                const name = match[1];
                const defStr = trimmed.slice(match[0].length);
                const def = this._parseInlineValue(defStr);
                this.registry.register('s', name, (typeof def === 'object' && def !== null) ? def : {});
                continue;
            }

            // Response group definition (r:name ...)
            match = trimmed.match(/^r:(\S+)\s+/);
            if (match) {
                const name = match[1];
                const defStr = trimmed.slice(match[0].length);
                const def = this._parseResponseGroup(defStr);
                this.registry.register('r', name, def);
                continue;
            }

            // Security group definition (sec:name ...)
            match = trimmed.match(/^sec:(\S+)\s+/);
            if (match) {
                const name = match[1];
                const defStr = trimmed.slice(match[0].length);
                const def = this._parseInlineValue(defStr);
                this.registry.register('sec', name, (typeof def === 'object' && def !== null) ? def : {});
                continue;
            }

            // @use import
            if (trimmed.startsWith('@use ')) {
                if (!this.directives.imports) this.directives.imports = [];
                this.directives.imports.push(trimmed);
                continue;
            }

            // Body line
            const depth = this._calculateDepth(line);
            parsedLines.push({ depth, content: line.trimStart(), lineNum });
        }

        if (parsedLines.length === 0) return [];

        // Second pass: parse body with ref resolution
        const first = parsedLines[0];

        if (this._isArrayHeader(first.content)) {
            const header = this._parseArrayHeader(first.content);
            if (header.key === null) {
                return this._decodeArrayFromHeader(0, parsedLines);
            }
        }

        // Explicit empty object marker
        if (parsedLines.length === 1 && first.content === '{}') {
            return {};
        }

        if (parsedLines.length === 1 && !this._isKeyValueLine(first.content)) {
            const val = this._parsePrimitive(first.content);
            return Array.isArray(val) ? val : [val];
        }

        return this._decodeObject(0, parsedLines, 0);
    }

    // --- Directive parsing ---

    _parseDirective(line) {
        const directive = line.slice(2);
        if (directive.startsWith('enforce(') && directive.endsWith(')')) {
            this.directives.enforce = directive.slice(8, -1);
        } else {
            this.directives.mode = directive;
        }
    }

    // --- Response group parsing ---

    _parseResponseGroup(input) {
        input = input.trim();
        if (input[0] !== OBRACE) return {};

        const inner = this._extractBraceContent(input);
        const result = {};
        const parts = this._splitTopLevel(inner, COMMA);

        for (let part of parts) {
            part = part.trim();
            const m = part.match(/^(\d+):("(?:[^"\\]|\\.)*")\s*(?:@s:(\S+))?\s*(.*)$/);
            if (m) {
                const code = m[1];
                const desc = this._parseStringLiteral(m[2]);
                const entry = { description: desc };
                if (m[3]) entry.schemaRef = m[3];
                if (m[4]) {
                    const overridesStr = m[4].trim();
                    if (overridesStr !== '' && overridesStr[0] === OBRACE) {
                        entry.overrides = this._parseInlineValue(overridesStr);
                    }
                }
                result[code] = entry;
            }
        }

        return result;
    }

    // --- Inline value parsing ---

    _parseInlineValue(input) {
        input = input.trim();
        if (input === '') return '';

        // Object
        if (input[0] === OBRACE) {
            const inner = this._extractBraceContent(input);
            return this._parseInlineObject(inner);
        }

        // Array
        if (input[0] === OBRACK) {
            const close = this._findMatchingBracket(input, 0);
            if (close !== -1) {
                const inner = input.slice(1, close);
                const items = this._splitTopLevel(inner, COMMA);
                return items.map(i => this._parseInlineValue(i.trim()));
            }
        }

        // Reference
        if (input.startsWith('@s:') || input.startsWith('@r:') || input.startsWith('@sec:')) {
            return this._resolveReference(input);
        }

        return this._parsePrimitive(input);
    }

    _parseInlineObject(inner) {
        const result = {};
        const parts = this._splitTopLevel(inner, COMMA);

        for (let part of parts) {
            part = part.trim();
            if (part === '') continue;

            const colonPos = this._findKeyColon(part);
            if (colonPos === -1) continue;

            let key = part.slice(0, colonPos).trim();
            const val = part.slice(colonPos + 1).trim();

            key = this._parseStringLiteral(key);

            if (key.includes('.')) {
                this._setDotPath(result, key, this._parseInlineValue(val));
            } else {
                result[key] = this._parseInlineValue(val);
            }
        }

        return result;
    }

    // --- Reference resolution ---

    _resolveReference(refStr) {
        // Polymorphic: @s:a | @s:b
        if (refStr.includes(' | ')) {
            const refs = [];
            for (let r of refStr.split(' | ')) {
                r = r.trim();
                const m = r.match(/^@(s|r|sec):(\S+)/);
                if (m) refs.push({ type: m[1], name: m[2] });
            }
            return this.registry.resolvePolymorphic(refs);
        }

        const m = refStr.match(/^@(s|r|sec):(\S+)\s*(.*)$/);
        if (m) {
            const type = m[1];
            const name = m[2];
            const rest = (m[3] || '').trim();

            if (rest !== '' && rest[0] === OBRACE) {
                const overrides = this._parseInlineValue(rest);
                return this.registry.resolveWithOverride(type, name, typeof overrides === 'object' && overrides !== null ? overrides : {});
            }

            return this.registry.resolve(type, name);
        }

        return refStr;
    }

    // --- Minification detection ---

    _isMinified(str) {
        return !str.includes('\n') && str.includes(SEMICOLON);
    }

    // --- Body parsing ---

    _calculateDepth(line) {
        let spaces = 0;
        for (let i = 0; i < line.length; i++) {
            if (line[i] === ' ') spaces++;
            else if (line[i] === '\t') throw new Error('Tabs not allowed for indentation');
            else break;
        }
        if (this.indent > 0 && spaces % this.indent !== 0) {
            throw new Error(`Invalid indentation: ${spaces} spaces (indent=${this.indent})`);
        }
        return this.indent > 0 ? spaces / this.indent : 0;
    }

    _decodeObject(baseDepth, parsedLines, startIndex) {
        const result = {};
        let i = startIndex;

        while (i < parsedLines.length) {
            const line = parsedLines[i];
            if (line.depth < baseDepth) break;
            if (line.depth > baseDepth) { i++; continue; }

            const content = line.content;

            // Array header
            if (this._isArrayHeader(content)) {
                const header = this._parseArrayHeader(content);
                if (header.key !== null) {
                    result[header.key] = this._decodeArrayFromHeader(i, parsedLines);
                    i++;
                    while (i < parsedLines.length && parsedLines[i].depth > baseDepth) i++;
                    continue;
                }
            }

            // Key-value
            if (this._isKeyValueLine(content)) {
                const [key, value, nextIndex] = this._decodeKeyValue(line, parsedLines, i, baseDepth);
                result[key] = value;
                i = nextIndex;
                continue;
            }

            i++;
        }

        return result;
    }

    _decodeKeyValue(line, parsedLines, index, baseDepth) {
        const content = line.content;
        const keyData = this._parseKey(content);
        const key = keyData.key;
        const rest = content.slice(keyData.end).trim();

        // Reference value
        if (rest !== '' && rest.startsWith('@')) {
            return [key, this._resolveReference(rest), index + 1];
        }

        if (rest !== '') {
            return [key, this._parsePrimitive(rest), index + 1];
        }

        // Nested object
        if (index + 1 < parsedLines.length && parsedLines[index + 1].depth > baseDepth) {
            const value = this._decodeObject(baseDepth + 1, parsedLines, index + 1);
            let nextIndex = index + 1;
            while (nextIndex < parsedLines.length && parsedLines[nextIndex].depth > baseDepth) {
                nextIndex++;
            }
            return [key, value, nextIndex];
        }

        return [key, [], index + 1];
    }

    _decodeArrayFromHeader(index, parsedLines) {
        const line = parsedLines[index];
        const header = this._parseArrayHeader(line.content);
        const baseDepth = line.depth;

        if (header.length === 0) return [];

        if (header.inlineValues !== null && header.fields === null) {
            return this._parseDelimitedValues(header.inlineValues, header.delimiter);
        }

        if (header.fields !== null) {
            return this._decodeTabularArray(index, parsedLines, baseDepth, header.length, header.fields, header.delimiter);
        }

        return this._decodeExpandedArray(index, parsedLines, baseDepth, header.length);
    }

    _decodeTabularArray(headerIdx, parsedLines, baseDepth, expected, fields, delim) {
        const result = [];
        let i = headerIdx + 1;

        while (i < parsedLines.length && result.length < expected) {
            if (parsedLines[i].depth !== baseDepth + 1) break;
            const values = this._parseDelimitedValues(parsedLines[i].content, delim);
            const row = {};
            for (let j = 0; j < fields.length; j++) {
                row[fields[j]] = values[j] ?? null;
            }
            result.push(row);
            i++;
        }

        return result;
    }

    _decodeExpandedArray(headerIdx, parsedLines, baseDepth, expected) {
        const result = [];
        let i = headerIdx + 1;

        while (i < parsedLines.length && result.length < expected) {
            const line = parsedLines[i];
            if (line.depth !== baseDepth + 1) break;

            if (line.content.startsWith(LIST_PREFIX)) {
                const itemContent = line.content.slice(LIST_PREFIX.length);

                // Schema/response/security reference as list item
                if (itemContent.startsWith('@s:') || itemContent.startsWith('@r:') || itemContent.startsWith('@sec:')) {
                    result.push(this._resolveReference(itemContent));
                    i++;
                    continue;
                }

                if (this._isKeyValueLine(itemContent)) {
                    const obj = this._decodeListItemObject(line, parsedLines, i, baseDepth);
                    result.push(obj);
                    i++;
                    while (i < parsedLines.length && parsedLines[i].depth > baseDepth + 1) i++;
                    continue;
                }

                if (this._isArrayHeader(itemContent)) {
                    const itemHeader = this._parseArrayHeader(itemContent);
                    if (itemHeader.inlineValues !== null) {
                        result.push(this._parseDelimitedValues(itemHeader.inlineValues, itemHeader.delimiter));
                    }
                } else {
                    result.push(this._parsePrimitive(itemContent));
                }
            }
            i++;
        }

        return result;
    }

    _decodeListItemObject(line, parsedLines, index, baseDepth) {
        const itemContent = line.content.slice(LIST_PREFIX.length);
        const keyData = this._parseKey(itemContent);
        const key = keyData.key;
        const rest = itemContent.slice(keyData.end).trim();

        const result = {};
        const contDepth = baseDepth + 2;

        if (rest !== '' && rest.startsWith('@')) {
            result[key] = this._resolveReference(rest);
        } else if (rest !== '') {
            result[key] = this._parsePrimitive(rest);
        } else if (index + 1 < parsedLines.length && parsedLines[index + 1].depth >= contDepth) {
            result[key] = this._decodeObject(contDepth, parsedLines, index + 1);
        } else {
            result[key] = [];
        }

        // Parse continuation fields
        let i = index + 1;
        while (i < parsedLines.length) {
            const nextLine = parsedLines[i];
            if (nextLine.depth < contDepth) break;
            if (nextLine.depth === contDepth) {
                if (nextLine.content.startsWith(LIST_PREFIX)) break;

                // Array header in continuation
                if (this._isArrayHeader(nextLine.content)) {
                    const header = this._parseArrayHeader(nextLine.content);
                    if (header.key !== null) {
                        result[header.key] = this._decodeArrayFromHeader(i, parsedLines);
                        i++;
                        while (i < parsedLines.length && parsedLines[i].depth > contDepth) i++;
                        continue;
                    }
                }
                if (this._isKeyValueLine(nextLine.content)) {
                    const [k, v, nextIdx] = this._decodeKeyValue(nextLine, parsedLines, i, contDepth);
                    result[k] = v;
                    i = nextIdx;
                    continue;
                }
            }
            i++;
        }

        return result;
    }

    // --- Parsing helpers ---

    _parseArrayHeader(content) {
        let key = null;
        const bracketStart = content.indexOf(OBRACK);

        if (bracketStart > 0) {
            const rawKey = content.slice(0, bracketStart).trim();
            key = this._parseStringLiteral(rawKey);
        }

        const bracketEnd = content.indexOf(CBRACK, bracketStart);
        if (bracketEnd === -1) throw new Error('Invalid array header: missing ]');

        let bracketContent = content.slice(bracketStart + 1, bracketEnd);

        let delimiter = COMMA;
        if (bracketContent.endsWith(TAB)) {
            delimiter = TAB;
            bracketContent = bracketContent.slice(0, -1);
        } else if (bracketContent.endsWith(PIPE)) {
            delimiter = PIPE;
            bracketContent = bracketContent.slice(0, -1);
        }

        const length = parseInt(bracketContent, 10);
        let fields = null;
        let braceStart = content.indexOf(OBRACE, bracketEnd);
        let colonIndex = content.indexOf(COLON, bracketEnd);

        if (braceStart !== -1 && (colonIndex === -1 || braceStart < colonIndex)) {
            const braceEnd = content.indexOf(CBRACE, braceStart);
            if (braceEnd !== -1) {
                const fieldsContent = content.slice(braceStart + 1, braceEnd);
                fields = this._parseDelimitedValues(fieldsContent, delimiter);
                colonIndex = content.indexOf(COLON, braceEnd);
            }
        }

        let inlineValues = null;
        if (colonIndex !== -1) {
            const afterColon = content.slice(colonIndex + 1).trim();
            if (afterColon !== '') {
                inlineValues = afterColon;
            }
        }

        return { key, length, delimiter, fields, inlineValues };
    }

    _parseDelimitedValues(input, delimiter) {
        const values = [];
        let buffer = '';
        let inQuotes = false;
        let braceDepth = 0;

        for (let i = 0; i < input.length; i++) {
            const char = input[i];

            if (char === BACKSLASH && inQuotes && i + 1 < input.length) {
                buffer += char + input[i + 1];
                i++;
                continue;
            }

            if (char === DQUOTE) {
                inQuotes = !inQuotes;
                buffer += char;
                continue;
            }

            if (!inQuotes) {
                if (char === OBRACE) braceDepth++;
                if (char === CBRACE) braceDepth--;
            }

            if (char === delimiter && !inQuotes && braceDepth === 0) {
                values.push(this._parsePrimitive(buffer.trim()));
                buffer = '';
                continue;
            }

            buffer += char;
        }

        if (buffer !== '' || values.length > 0) {
            values.push(this._parsePrimitive(buffer.trim()));
        }

        return values;
    }

    _parsePrimitive(token) {
        const trimmed = token.trim();
        if (trimmed === '') return '';
        if (trimmed === '[]') return [];

        if (trimmed[0] === DQUOTE) {
            return this._parseStringLiteral(trimmed);
        }

        if (trimmed === 'true') return true;
        if (trimmed === 'false') return false;
        if (trimmed === 'null') return null;

        // Strict decimal regex matching PHP is_numeric (no hex, no binary, no Infinity)
        if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(trimmed)) {
            if (trimmed.includes('.') || trimmed.includes('e') || trimmed.includes('E')) {
                return parseFloat(trimmed);
            }
            return parseInt(trimmed, 10);
        }

        return trimmed;
    }

    _parseStringLiteral(token) {
        const trimmed = token.trim();
        if (trimmed === '' || trimmed[0] !== DQUOTE) return trimmed;

        const closingQuote = this._findClosingQuote(trimmed, 0);
        if (closingQuote === -1) throw new Error('Unterminated string');

        return this._unescapeString(trimmed.slice(1, closingQuote));
    }

    _findClosingQuote(str, start) {
        let i = start + 1;
        while (i < str.length) {
            if (str[i] === BACKSLASH && i + 1 < str.length) { i += 2; continue; }
            if (str[i] === DQUOTE) return i;
            i++;
        }
        return -1;
    }

    _unescapeString(str) {
        let result = str.replace(/\\\\/g, '\x00BACKSLASH\x00');
        result = result.replace(/\\"/g, '"');
        result = result.replace(/\\n/g, '\n');
        result = result.replace(/\\r/g, '\r');
        result = result.replace(/\\t/g, '\t');
        result = result.replace(/\\;/g, ';');
        result = result.replace(/\x00BACKSLASH\x00/g, '\\');
        return result;
    }

    _parseKey(content) {
        if (content[0] === DQUOTE) {
            const closingQuote = this._findClosingQuote(content, 0);
            if (closingQuote === -1) throw new Error('Unterminated quoted key');
            const key = this._unescapeString(content.slice(1, closingQuote));
            const end = closingQuote + 1;
            if (end >= content.length || content[end] !== COLON) {
                throw new Error('Missing colon after key');
            }
            return { key, end: end + 1 };
        }

        const colonPos = content.indexOf(COLON);
        if (colonPos === -1) throw new Error('Missing colon after key');

        return { key: content.slice(0, colonPos).trim(), end: colonPos + 1 };
    }

    _findKeyColon(str) {
        let inQuotes = false;
        let braceDepth = 0;

        for (let i = 0; i < str.length; i++) {
            const char = str[i];
            if (char === BACKSLASH && inQuotes && i + 1 < str.length) { i++; continue; }
            if (char === DQUOTE) { inQuotes = !inQuotes; continue; }
            if (!inQuotes) {
                if (char === OBRACE) braceDepth++;
                if (char === CBRACE) braceDepth--;
                if (char === COLON && braceDepth === 0) return i;
            }
        }

        return -1;
    }

    _splitTopLevel(input, delimiter) {
        const parts = [];
        let buffer = '';
        let inQuotes = false;
        let braceDepth = 0;
        let bracketDepth = 0;

        for (let i = 0; i < input.length; i++) {
            const char = input[i];
            if (char === BACKSLASH && inQuotes && i + 1 < input.length) {
                buffer += char + input[i + 1];
                i++;
                continue;
            }
            if (char === DQUOTE) inQuotes = !inQuotes;
            if (!inQuotes) {
                if (char === OBRACE) braceDepth++;
                if (char === CBRACE) braceDepth--;
                if (char === OBRACK) bracketDepth++;
                if (char === CBRACK) bracketDepth--;
            }
            if (char === delimiter && !inQuotes && braceDepth === 0 && bracketDepth === 0) {
                parts.push(buffer);
                buffer = '';
                continue;
            }
            buffer += char;
        }

        if (buffer !== '') parts.push(buffer);
        return parts;
    }

    _findMatchingBracket(str, start) {
        let depth = 0;
        let inQuotes = false;

        for (let i = start; i < str.length; i++) {
            const char = str[i];
            if (char === BACKSLASH && inQuotes && i + 1 < str.length) { i++; continue; }
            if (char === DQUOTE) { inQuotes = !inQuotes; continue; }
            if (!inQuotes) {
                if (char === OBRACK) depth++;
                if (char === CBRACK) {
                    depth--;
                    if (depth === 0) return i;
                }
            }
        }

        return -1;
    }

    _extractBraceContent(input) {
        let depth = 0;
        let start = -1;
        let inQuotes = false;

        for (let i = 0; i < input.length; i++) {
            const char = input[i];
            if (char === BACKSLASH && inQuotes && i + 1 < input.length) { i++; continue; }
            if (char === DQUOTE) { inQuotes = !inQuotes; continue; }
            if (!inQuotes) {
                if (char === OBRACE) {
                    if (depth === 0) start = i;
                    depth++;
                }
                if (char === CBRACE) {
                    depth--;
                    if (depth === 0) {
                        return input.slice(start + 1, i);
                    }
                }
            }
        }

        return '';
    }

    _isArrayHeader(content) {
        const bracketPos = content.indexOf(OBRACK);
        const colonPos = content.indexOf(COLON);
        return bracketPos !== -1 && colonPos !== -1 && bracketPos < colonPos;
    }

    _isKeyValueLine(content) {
        return content.includes(COLON);
    }

    _setDotPath(obj, path, val) {
        const keys = path.split('.');
        let ref = obj;
        for (let i = 0; i < keys.length; i++) {
            if (i === keys.length - 1) {
                ref[keys[i]] = val;
            } else {
                if (!Object.hasOwn(ref, keys[i]) || typeof ref[keys[i]] !== 'object' || ref[keys[i]] === null) {
                    ref[keys[i]] = {};
                }
                ref = ref[keys[i]];
            }
        }
    }
}
