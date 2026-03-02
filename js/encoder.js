// js/encoder.js
// Port of bX\Scon\Encoder — JS data → SCON string

import { SchemaRegistry } from './schema-registry.js';
import { TreeHash } from './tree-hash.js';

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
const HEADER = '#!scon/1.0';

export class Encoder {

    constructor(options = {}) {
        this.indentSize = options.indent ?? 1;
        this.delimiter = options.delimiter ?? COMMA;
        this.mode = options.mode ?? 'warn';
        this.enforce = options.enforce ?? null;
        this.autoExtract = options.autoExtract ?? false;
        this.header = options.header ?? false; // header off by default
        this.registry = new SchemaRegistry();
        this.warnings = [];
    }

    getRegistry() { return this.registry; }
    getWarnings() { return this.warnings; }

    // Encode JS data to SCON string
    // Returns string if sync (no autoExtract), Promise<string> if autoExtract
    encode(data, schemas = {}, responses = {}, security = {}) {
        // Register explicit schemas
        for (const [name, def] of Object.entries(schemas)) {
            this.registry.register('s', name, def);
        }
        for (const [name, def] of Object.entries(responses)) {
            this.registry.register('r', name, def);
        }
        for (const [name, def] of Object.entries(security)) {
            this.registry.register('sec', name, def);
        }

        if (this.autoExtract && typeof data === 'object' && data !== null) {
            // async path — needs TreeHash
            return this._encodeAsync(data);
        }

        return this._encodeFinal(data);
    }

    async _encodeAsync(data) {
        await this._detectRepeatedSchemas(data);
        return this._encodeFinal(data);
    }

    _encodeFinal(data) {
        const lines = [];

        // Header (optional, off by default)
        if (this.header) {
            lines.push(HEADER);
        }

        // Directives
        if (this.mode !== 'warn') {
            lines.push(`@@${this.mode}`);
        }
        if (this.enforce !== null) {
            lines.push(`@@enforce(${this.enforce})`);
        }

        // Schema definitions
        const allSchemas = this.registry.getAll('s');
        const schemaKeys = Object.keys(allSchemas);
        if (schemaKeys.length > 0) {
            if (lines.length > 0) lines.push('');
            for (const name of schemaKeys) {
                lines.push(`s:${name} ${this._encodeInline(allSchemas[name])}`);
            }
        }

        // Response group definitions
        const allResponses = this.registry.getAll('r');
        const responseKeys = Object.keys(allResponses);
        if (responseKeys.length > 0) {
            if (lines.length > 0) lines.push('');
            for (const name of responseKeys) {
                lines.push(`r:${name} ${this._encodeResponseGroup(allResponses[name])}`);
            }
        }

        // Security group definitions
        const allSecurity = this.registry.getAll('sec');
        const securityKeys = Object.keys(allSecurity);
        if (securityKeys.length > 0) {
            if (lines.length > 0) lines.push('');
            for (const name of securityKeys) {
                lines.push(`sec:${name} ${this._encodeInline(allSecurity[name])}`);
            }
        }

        // Separator before body
        if (schemaKeys.length > 0 || responseKeys.length > 0 || securityKeys.length > 0) {
            lines.push('');
        }

        // Body — explicit {} for empty object (preserves type distinction vs [])
        if (typeof data === 'object' && data !== null && !Array.isArray(data) && Object.keys(data).length === 0) {
            lines.push('{}');
        } else {
            for (const line of this._encodeValue(data, 0)) {
                lines.push(line);
            }
        }

        // Prune orphan schemas
        if (this.autoExtract && schemaKeys.length > 0) {
            const body = lines.join('\n');
            const pruned = lines.filter(line => {
                const m = line.match(/^s:(\S+)\s/);
                if (m && !body.includes('@s:' + m[1])) return false;
                return true;
            });
            return pruned.join('\n');
        }

        return lines.join('\n');
    }

    // --- Response group encoding ---

    _encodeResponseGroup(group) {
        const parts = [];
        for (const [code, def] of Object.entries(group)) {
            const desc = def.description || '';
            const schemaRef = def.schemaRef || null;
            let part = `${code}:${this._encodeString(desc)}`;
            if (schemaRef !== null) {
                part += ` @s:${schemaRef}`;
                if (def.overrides && Object.keys(def.overrides).length > 0) {
                    part += ' ' + this._encodeInline(def.overrides);
                }
            }
            parts.push(part);
        }
        return OBRACE + parts.join(', ') + CBRACE;
    }

    // --- Inline encoding ---

    _encodeInline(data) {
        if (this._isPrimitive(data)) {
            return this._encodePrimitive(data);
        }

        if (Array.isArray(data)) {
            const items = data.map(v => this._encodeInline(v));
            return OBRACK + items.join(', ') + CBRACK;
        }

        if (typeof data === 'object' && data !== null) {
            const parts = [];
            for (const [key, val] of Object.entries(data)) {
                parts.push(`${this._encodeKey(key)}:${this._encodeInline(val)}`);
            }
            return OBRACE + parts.join(', ') + CBRACE;
        }

        return '';
    }

    // --- Value encoding with indentation ---

    *_encodeValue(value, depth) {
        if (this._isPrimitive(value)) {
            const encoded = this._encodePrimitive(value);
            if (encoded !== '') yield encoded;
            return;
        }

        if (Array.isArray(value)) {
            yield* this._encodeArray(null, value, depth);
        } else if (typeof value === 'object' && value !== null) {
            yield* this._encodeObject(value, depth);
        }
    }

    *_encodeObject(obj, depth) {
        for (const [key, val] of Object.entries(obj)) {
            if (this._isPrimitive(val)) {
                yield this._indented(depth, `${this._encodeKey(key)}: ${this._encodePrimitive(val)}`);
            } else if (Array.isArray(val)) {
                yield* this._encodeArray(key, val, depth);
            } else if (typeof val === 'object' && val !== null) {
                const schemaRef = this._findMatchingSchema(val);
                if (schemaRef !== null) {
                    yield this._indented(depth, `${this._encodeKey(key)}: @s:${schemaRef}`);
                } else {
                    yield this._indented(depth, `${this._encodeKey(key)}:`);
                    if (Object.keys(val).length > 0) {
                        yield* this._encodeObject(val, depth + 1);
                    }
                }
            }
        }
    }

    *_encodeArray(key, array, depth) {
        const length = array.length;

        if (length === 0) {
            if (key !== null) {
                yield this._indented(depth, `${this._encodeKey(key)}: []`);
            } else {
                yield this._indented(depth, '[]');
            }
            return;
        }

        // Array of primitives
        if (this._isArrayOfPrimitives(array)) {
            const header = this._formatHeader(length, key);
            const values = this._joinPrimitives(array);
            yield this._indented(depth, `${header} ${values}`);
            return;
        }

        // Array of objects (tabular)
        if (this._isArrayOfObjects(array)) {
            const fields = this._extractTabularHeader(array);
            if (fields !== null) {
                yield* this._encodeTabularArray(key, array, fields, depth);
                return;
            }
        }

        // Mixed / expanded array
        yield* this._encodeMixedArray(key, array, depth);
    }

    *_encodeTabularArray(key, rows, fields, depth) {
        const header = this._formatHeader(rows.length, key, fields);
        yield this._indented(depth, header);

        for (const row of rows) {
            const values = fields.map(f => row[f] ?? null);
            yield this._indented(depth + 1, this._joinPrimitives(values));
        }
    }

    *_encodeMixedArray(key, items, depth) {
        const header = this._formatHeader(items.length, key);
        yield this._indented(depth, header);

        for (const item of items) {
            if (this._isPrimitive(item)) {
                yield this._listItem(depth + 1, this._encodePrimitive(item));
            } else if (typeof item === 'object' && item !== null && !Array.isArray(item)) {
                const schemaRef = this._findMatchingSchema(item);
                if (schemaRef !== null) {
                    yield this._listItem(depth + 1, `@s:${schemaRef}`);
                } else {
                    yield* this._encodeObjectAsListItem(item, depth + 1);
                }
            } else if (Array.isArray(item)) {
                if (item.length === 0) {
                    yield this._listItem(depth + 1, '[]');
                } else if (this._isArrayOfPrimitives(item)) {
                    const subHeader = this._formatHeader(item.length, null);
                    const values = this._joinPrimitives(item);
                    yield this._listItem(depth + 1, `${subHeader} ${values}`);
                }
            }
        }
    }

    *_encodeObjectAsListItem(obj, depth) {
        const keys = Object.keys(obj);
        if (keys.length === 0) {
            yield this._indented(depth, LIST_PREFIX);
            return;
        }

        const firstKey = keys[0];
        const firstVal = obj[firstKey];
        const rest = {};
        for (let i = 1; i < keys.length; i++) {
            rest[keys[i]] = obj[keys[i]];
        }

        const encodedKey = this._encodeKey(firstKey);

        if (this._isPrimitive(firstVal)) {
            yield this._listItem(depth, `${encodedKey}: ${this._encodePrimitive(firstVal)}`);
        } else if (Array.isArray(firstVal) && firstVal.length === 0) {
            yield this._listItem(depth, `${encodedKey}: []`);
        } else if (Array.isArray(firstVal) && this._isArrayOfPrimitives(firstVal)) {
            const hdr = this._formatHeader(firstVal.length, null);
            const vals = this._joinPrimitives(firstVal);
            yield this._listItem(depth, `${encodedKey}${hdr} ${vals}`);
        } else if (typeof firstVal === 'object' && firstVal !== null) {
            yield this._listItem(depth, `${encodedKey}:`);
            yield* this._encodeObject(firstVal, depth + 2);
        }

        if (Object.keys(rest).length > 0) {
            yield* this._encodeObject(rest, depth + 1);
        }
    }

    // --- Schema matching ---

    _findMatchingSchema(data) {
        const allSchemas = this.registry.getAll('s');
        for (const [name, def] of Object.entries(allSchemas)) {
            if (this._deepEqual(data, def)) return name;
        }
        return null;
    }

    // Order-sensitive deep equality (matches PHP === behavior)
    _deepEqual(a, b) {
        if (a === b) return true;
        if (typeof a !== typeof b) return false;
        if (a === null || b === null) return a === b;
        if (Array.isArray(a) !== Array.isArray(b)) return false;
        if (Array.isArray(a)) {
            if (a.length !== b.length) return false;
            return a.every((v, i) => this._deepEqual(v, b[i]));
        }
        if (typeof a === 'object') {
            const keysA = Object.keys(a);
            const keysB = Object.keys(b);
            if (keysA.length !== keysB.length) return false;
            // Key order must match (PHP === is order-sensitive for assoc arrays)
            for (let i = 0; i < keysA.length; i++) {
                if (keysA[i] !== keysB[i]) return false;
            }
            return keysA.every(k => this._deepEqual(a[k], b[k]));
        }
        return false;
    }

    // --- Auto-extract repeated schemas via TreeHash ---

    async _detectRepeatedSchemas(data) {
        const result = await TreeHash.hashTree(data, '', 2, false);

        for (const entry of Object.values(result.index)) {
            if (entry.count >= 2) {
                const name = await this._generateSchemaName(entry.path);
                this.registry.register('s', name, entry.data);
            }
        }
    }

    async _generateSchemaName(path) {
        let parts = path.replace(/^\./, '').split('.');
        // Strip list indices from the end
        while (parts.length > 0 && /^\[\d+\]$/.test(parts[parts.length - 1])) {
            parts.pop();
        }
        let last = parts[parts.length - 1] || '';
        last = last.replace(/properties|content|application\/json|schema/g, '').trim().replace(/^\.+|\.+$/g, '');
        if (!last) {
            // Use xxh128 matching PHP substr(hash('xxh128', $path), 0, 6)
            const hash = await TreeHash.hash(path);
            last = 'auto_' + hash.slice(0, 6);
        }
        return last;
    }

    // --- Formatting helpers ---

    _formatHeader(length, key = null, fields = null) {
        let header = '';
        if (key !== null) {
            header += this._encodeKey(key);
        }

        const delimSuffix = this.delimiter !== COMMA ? this.delimiter : '';
        header += OBRACK + length + delimSuffix + CBRACK;

        if (fields !== null) {
            const qFields = fields.map(f => this._encodeKey(f));
            header += OBRACE + qFields.join(this.delimiter) + CBRACE;
        }

        header += COLON;
        return header;
    }

    _extractTabularHeader(array) {
        if (array.length === 0) return null;
        const first = array[0];
        if (typeof first !== 'object' || first === null || Array.isArray(first)) return null;

        const firstKeys = Object.keys(first);
        if (firstKeys.length === 0) return null;

        for (const row of array) {
            if (typeof row !== 'object' || row === null || Array.isArray(row)) return null;
            const rowKeys = Object.keys(row);
            if (rowKeys.length !== firstKeys.length) return null;
            for (const fk of firstKeys) {
                if (!(fk in row)) return null;
                if (!this._isPrimitive(row[fk])) return null;
            }
        }

        return firstKeys;
    }

    // --- Primitive encoding ---

    _encodePrimitive(value) {
        if (value === null) return 'null';
        if (value === true) return 'true';
        if (value === false) return 'false';
        if (typeof value === 'number') return String(value);
        if (typeof value === 'string') return this._encodeString(value);
        return '';
    }

    _encodeString(value) {
        if (this._isSafeUnquoted(value)) return value;
        if (value.length > 0 && (value[0] === '{' || value[0] === '[')) {
            this.warnings.push(`String starts with '${value[0]}', possible incomplete structure: ${value.slice(0, 60)}`);
        }
        return DQUOTE + this._escapeString(value) + DQUOTE;
    }

    _encodeKey(key) {
        if (this._isValidUnquotedKey(key)) return key;
        return DQUOTE + this._escapeString(key) + DQUOTE;
    }

    _escapeString(str) {
        let escaped = str.replace(/\\/g, '\\\\');
        escaped = escaped.replace(/"/g, '\\"');
        escaped = escaped.replace(/\n/g, '\\n');
        escaped = escaped.replace(/\r/g, '\\r');
        escaped = escaped.replace(/\t/g, '\\t');
        escaped = escaped.replace(/;/g, '\\;');
        return escaped;
    }

    _isSafeUnquoted(value) {
        if (value === '') return false;
        if (value === 'true' || value === 'false' || value === 'null') return false;
        // Strict decimal check matching PHP is_numeric
        if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(value)) return false;
        if (value.includes(this.delimiter)) return false;
        if (/[\s:"\\;@#\{\[\]\}]/.test(value)) return false;
        return true;
    }

    _isValidUnquotedKey(key) {
        if (key === '') return false;
        if (key[0] === '#') return false; // starts-with-# looks like comment
        if (/[:\[\]{}"\\\s;@#,]/.test(key)) return false;
        return true;
    }

    _joinPrimitives(values) {
        return values.map(v => this._encodePrimitive(v)).join(this.delimiter + ' ');
    }

    _isPrimitive(value) {
        return value === null || typeof value === 'boolean' || typeof value === 'number' || typeof value === 'string';
    }

    _isArrayOfPrimitives(arr) {
        if (!Array.isArray(arr)) return false;
        return arr.every(item => this._isPrimitive(item));
    }

    _isArrayOfObjects(arr) {
        if (!Array.isArray(arr)) return false;
        return arr.every(item => typeof item === 'object' && item !== null && !Array.isArray(item));
    }

    _indented(depth, content) {
        return ' '.repeat(this.indentSize * depth) + content;
    }

    _listItem(depth, content) {
        return this._indented(depth, LIST_PREFIX + content);
    }
}
