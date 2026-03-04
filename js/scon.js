// js/scon.js
// S.C.O.N. — Schema-Compact Object Notation
// Extension: .scon | Content-Type: text/scon; charset=utf-8
//
// WASM-accelerated: loads Rust tape decoder via WebAssembly when available,
// falls back to pure JS implementation transparently.
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

// WASM module — lazy-loaded, null until init
let wasmModule = null;
let wasmInitPromise = null;

async function loadWasm() {
    try {
        const wasm = await import('../wasm/pkg/scon_wasm.js');
        const wasmUrl = new URL('../wasm/pkg/scon_wasm_bg.wasm', import.meta.url);
        await wasm.default(wasmUrl);
        wasmModule = wasm;
    } catch {
        // WASM not available — pure JS fallback
        wasmModule = null;
    }
}

// Trigger WASM load on import (non-blocking)
function ensureWasm() {
    if (!wasmInitPromise) {
        wasmInitPromise = loadWasm();
    }
    return wasmInitPromise;
}

// Start loading immediately
ensureWasm();

// Encode JS data to SCON string
// Returns string (sync) or Promise<string> if options.autoExtract is true
function encode(data, options = {}) {
    // WASM fast path — no schema/response/security options (those are JS-only features)
    if (wasmModule && !options.autoExtract && !options.schemas && !options.responses && !options.security) {
        const indent = options.indent ?? 1;
        return indent > 1
            ? wasmModule.scon_encode_indent(data, indent)
            : wasmModule.scon_encode(data);
    }
    const encoder = new Encoder(options);
    const schemas = options.schemas || {};
    const responses = options.responses || {};
    const security = options.security || {};
    return encoder.encode(data, schemas, responses, security);
}

// Decode SCON string to JS object
function decode(sconString, options = {}) {
    // WASM fast path — single crossing: tape→JSON string in WASM, JSON.parse in V8 native C++
    if (wasmModule && !options.schemas && !options.responses && !options.security) {
        return JSON.parse(wasmModule.scon_to_json(sconString));
    }
    const decoder = new Decoder(options);
    return decoder.decode(sconString);
}

// Minify SCON string to single line
function minify(sconString) {
    if (wasmModule) {
        return wasmModule.scon_minify(sconString);
    }
    return Minifier.minify(sconString);
}

// Expand minified SCON to indented format
function expand(minifiedString, options = {}) {
    const indent = options.indent ?? 1;
    if (wasmModule) {
        return wasmModule.scon_expand(minifiedString, indent);
    }
    return Minifier.expand(minifiedString, indent);
}

// Validate SCON data against rules (JS-only, no WASM equivalent)
function validate(data, options = {}) {
    const validator = new Validator(options);
    return validator.validate(data);
}

// Wait for WASM to be ready (optional — operations work without it via fallback)
async function ready() {
    await ensureWasm();
    return wasmModule !== null;
}

const SCON = Object.freeze({
    encode,
    decode,
    minify,
    expand,
    validate,
    ready,
    // Aliases estilo JSON
    parse: decode,
    stringify: encode,
});

export default SCON;
export { SCON, Encoder, Decoder, Minifier, Validator };
export { SchemaRegistry } from './schema-registry.js';
export { TreeHash } from './tree-hash.js';
