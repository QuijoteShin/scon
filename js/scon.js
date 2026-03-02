// js/scon.js
// S.C.O.N. — Schema-Compact Object Notation
// Extension: .scon | Content-Type: text/scon; charset=utf-8
//
// Performance comparison (OpenAPI 3.1 spec, 71 endpoints):
//
// Format           | Bytes   | Ratio | Gzip    |
// -----------------|---------|-------|---------|
// JSON             | 90,886  | 1.00x |  4,632  |
// SCON             | 26,347  | 0.29x |  3,969  |
// SCON (minified)  | 20,211  | 0.22x |  3,818  |
//
// SCON achieves ~71% reduction vs JSON by extracting repeated schema
// definitions (s:, r:, sec:) and referencing them (@s:).

import { Encoder } from './encoder.js';
import { Decoder } from './decoder.js';
import { Minifier } from './minifier.js';
import { Validator } from './validator.js';

// Encode JS data to SCON string (1:1 with Scon::encode)
// Returns string (sync) or Promise<string> if options.autoExtract is true
function encode(data, options = {}) {
    const encoder = new Encoder(options);
    const schemas = options.schemas || {};
    const responses = options.responses || {};
    const security = options.security || {};
    return encoder.encode(data, schemas, responses, security);
}

// Decode SCON string to JS object (1:1 with Scon::decode)
function decode(sconString, options = {}) {
    const decoder = new Decoder(options);
    return decoder.decode(sconString);
}

// Minify SCON string to single line (1:1 with Scon::minify)
function minify(sconString) {
    return Minifier.minify(sconString);
}

// Expand minified SCON to indented format (1:1 with Scon::expand)
function expand(minifiedString, options = {}) {
    return Minifier.expand(minifiedString, options.indent ?? 1);
}

// Validate SCON data against rules (1:1 with Scon::validate)
function validate(data, options = {}) {
    const validator = new Validator(options);
    return validator.validate(data);
}

const SCON = Object.freeze({
    encode,
    decode,
    minify,
    expand,
    validate,
    // Aliases estilo JSON
    parse: decode,
    stringify: encode,
});

export default SCON;
export { SCON, Encoder, Decoder, Minifier, Validator };
export { SchemaRegistry } from './schema-registry.js';
export { TreeHash } from './tree-hash.js';
