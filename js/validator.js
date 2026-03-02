// js/validator.js
// Port of bX\Scon\Validator — SCON data validation

const ENFORCE_RULES = {
    'openapi:3.1': {
        required: ['openapi', 'info', 'paths'],
        'info.required': ['title', 'version'],
    },
    'openapi:3.0': {
        required: ['openapi', 'info', 'paths'],
        'info.required': ['title', 'version'],
    },
};

export class Validator {

    constructor(options = {}) {
        this.mode = options.mode || 'warn';
        this.enforce = options.enforce || null;
        this.warnings = [];
        this.errors = [];
    }

    // Validate data and return result
    validate(data) {
        this.warnings = [];
        this.errors = [];

        if (typeof data !== 'object' || data === null) {
            this._addIssue('Root value must be an object or array');
            return this._result();
        }

        // Apply enforce rules if configured
        if (this.enforce !== null && this.enforce in ENFORCE_RULES) {
            this._enforceSpec(data, ENFORCE_RULES[this.enforce]);
        }

        return this._result();
    }

    // Validate a schema definition
    validateSchema(name, schema) {
        this.warnings = [];
        this.errors = [];

        if (!schema || Object.keys(schema).length === 0) {
            this._addIssue(`Schema '${name}' is empty`);
        }

        return this._result();
    }

    // Validate data against a schema (field presence)
    validateAgainstSchema(data, schema, path = '') {
        this.warnings = [];
        this.errors = [];

        for (const [key, def] of Object.entries(schema)) {
            const fieldPath = path ? `${path}.${key}` : key;
            const isRequired = key.endsWith('!');
            const cleanKey = key.replace(/!$/, '');

            // PHP isset() returns false for null — match that behavior
            if (isRequired && (data[cleanKey] === undefined || data[cleanKey] === null)) {
                this._addIssue(`Missing required field: ${fieldPath}`);
            }
        }

        // Check for extra fields
        const schemaKeys = Object.keys(schema).map(k => k.replace(/!$/, ''));
        if (this.mode === 'strict') {
            for (const dataKey of Object.keys(data)) {
                if (!schemaKeys.includes(dataKey)) {
                    this._addIssue(`Extra field not in schema: ${path}.${dataKey}`);
                }
            }
        } else if (this.mode === 'warn') {
            for (const dataKey of Object.keys(data)) {
                if (!schemaKeys.includes(dataKey)) {
                    this.warnings.push(`Extra field: ${path}.${dataKey}`);
                }
            }
        }

        return this._result();
    }

    _enforceSpec(data, rules) {
        if (rules.required) {
            for (const field of rules.required) {
                if (data[field] === undefined || data[field] === null) {
                    this._addIssue(`Missing required field per ${this.enforce}: ${field}`);
                }
            }
        }

        // Check nested required fields (pattern: parent.required)
        for (const [ruleKey, ruleFields] of Object.entries(rules)) {
            if (ruleKey === 'required') continue;
            if (ruleKey.endsWith('.required')) {
                const parentKey = ruleKey.slice(0, -9);
                if (data[parentKey] !== undefined && data[parentKey] !== null && typeof data[parentKey] === 'object') {
                    for (const field of ruleFields) {
                        if (data[parentKey][field] === undefined || data[parentKey][field] === null) {
                            this._addIssue(`Missing required field per ${this.enforce}: ${parentKey}.${field}`);
                        }
                    }
                }
            }
        }
    }

    _addIssue(msg) {
        if (this.mode === 'strict' || this.enforce !== null) {
            this.errors.push(msg);
        } else if (this.mode === 'warn') {
            this.warnings.push(msg);
        }
    }

    _result() {
        return {
            valid: this.errors.length === 0,
            warnings: this.warnings,
            errors: this.errors,
            mode: this.mode,
            enforce: this.enforce,
        };
    }
}
