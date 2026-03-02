// js/schema-registry.js
// Port of bX\Scon\SchemaRegistry — schema/response/security registration and resolution

export class SchemaRegistry {

    constructor() {
        this.schemas = {};
        this.responses = {};
        this.security = {};
        this.resolving = {}; // cycle detection
    }

    // Register a definition
    register(type, name, definition) {
        switch (type) {
            case 's':   this.schemas[name] = definition; break;
            case 'r':   this.responses[name] = definition; break;
            case 'sec': this.security[name] = definition; break;
            default: throw new Error(`Unknown definition type: ${type}`);
        }
    }

    // Resolve a reference by type and name
    resolve(type, name) {
        const store = this._getStore(type);
        if (!Object.hasOwn(store, name)) {
            throw new Error(`Undefined reference: @${type}:${name}`);
        }

        const refKey = `${type}:${name}`;
        if (Object.hasOwn(this.resolving, refKey)) {
            // Circular reference — return marker for lazy resolution
            return { '$ref': `#/definitions/${name}` };
        }

        this.resolving[refKey] = true;
        const resolved = this._deepResolveRefs(store[name]);
        delete this.resolving[refKey];

        return resolved;
    }

    // Resolve with override (deep merge + dot-notation + field removal)
    resolveWithOverride(type, name, overrides) {
        let base = this.resolve(type, name);

        // Process field removals first
        const removals = [];
        const merges = {};
        for (const key of Object.keys(overrides)) {
            if (key.startsWith('-')) {
                removals.push(key.slice(1));
            } else {
                merges[key] = overrides[key];
            }
        }

        // Apply removals
        for (const field of removals) {
            if (field.includes('.')) {
                this._unsetDotPath(base, field);
            } else {
                delete base[field];
            }
        }

        // Apply deep merges with dot-notation support
        for (const [key, val] of Object.entries(merges)) {
            if (key.includes('.')) {
                this._setDotPath(base, key, val);
            } else {
                if (
                    val !== null && typeof val === 'object' && !Array.isArray(val) &&
                    Object.hasOwn(base, key) && typeof base[key] === 'object' && base[key] !== null && !Array.isArray(base[key])
                ) {
                    base[key] = this._deepMerge(base[key], val);
                } else {
                    base[key] = val;
                }
            }
        }

        return base;
    }

    // Resolve polymorphic references (oneOf with pipe)
    resolvePolymorphic(refs) {
        const schemas = refs.map(ref => this.resolve(ref.type, ref.name));
        return { oneOf: schemas };
    }

    // Check if a definition exists
    has(type, name) {
        try {
            const store = this._getStore(type);
            return Object.hasOwn(store, name);
        } catch {
            return false;
        }
    }

    // Get all definitions of a type (for encoding)
    getAll(type) {
        try {
            return this._getStore(type);
        } catch {
            return {};
        }
    }

    // Reset all definitions
    reset() {
        this.schemas = {};
        this.responses = {};
        this.security = {};
        this.resolving = {};
    }

    // --- Private ---

    _getStore(type) {
        switch (type) {
            case 's':   return this.schemas;
            case 'r':   return this.responses;
            case 'sec': return this.security;
            default: throw new Error(`Unknown ref type: ${type}`);
        }
    }

    // Deep-resolve any @ref markers within a definition
    _deepResolveRefs(data) {
        if (data === null || typeof data !== 'object') return data;
        if (Array.isArray(data)) return data.map(item => this._deepResolveRefs(item));

        const result = {};
        for (const [key, val] of Object.entries(data)) {
            if (val !== null && typeof val === 'object' && !Array.isArray(val)) {
                if ('@ref' in val) {
                    const ref = val['@ref'];
                    if ('@overrides' in val) {
                        result[key] = this.resolveWithOverride(ref.type, ref.name, val['@overrides']);
                    } else {
                        result[key] = this.resolve(ref.type, ref.name);
                    }
                } else if ('@polymorphic' in val) {
                    result[key] = this.resolvePolymorphic(val['@polymorphic']);
                } else {
                    result[key] = this._deepResolveRefs(val);
                }
            } else {
                result[key] = val;
            }
        }
        return result;
    }

    // Set a value using dot-notation path (a.b.c = val)
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

    // Unset a value using dot-notation path
    _unsetDotPath(obj, path) {
        const keys = path.split('.');
        let ref = obj;
        for (let i = 0; i < keys.length; i++) {
            if (i === keys.length - 1) {
                delete ref[keys[i]];
            } else {
                if (!Object.hasOwn(ref, keys[i]) || typeof ref[keys[i]] !== 'object' || ref[keys[i]] === null) {
                    return;
                }
                ref = ref[keys[i]];
            }
        }
    }

    // Deep merge: objects merge recursively, arrays replace
    _deepMerge(base, override) {
        const result = { ...base };
        for (const [key, val] of Object.entries(override)) {
            if (
                val !== null && typeof val === 'object' && !Array.isArray(val) &&
                Object.hasOwn(result, key) && typeof result[key] === 'object' && result[key] !== null && !Array.isArray(result[key])
            ) {
                result[key] = this._deepMerge(result[key], val);
            } else {
                result[key] = val;
            }
        }
        return result;
    }
}
