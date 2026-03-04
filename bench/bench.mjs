#!/usr/bin/env node
// bench/bench.mjs
// SCON Benchmark Suite — JavaScript (Node.js)
// Usage: node bench/bench.mjs [--iterations=100]
//
// Datasets match PHP bench for cross-language comparison

import { createRequire } from 'module';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { gzipSync } from 'zlib';

const __dirname = dirname(fileURLToPath(import.meta.url));
const jsDir = join(__dirname, '..', 'js');

// Import SCON modules directly (skip scon.js to avoid tree-hash.js hash-wasm dep)
const { Encoder } = await import(join(jsDir, 'encoder.js'));
const { Decoder } = await import(join(jsDir, 'decoder.js'));
const { Minifier } = await import(join(jsDir, 'minifier.js'));

const SCON = {
    encode(data, options = {}) {
        const encoder = new Encoder(options);
        return encoder.encode(data, options.schemas || {}, options.responses || {}, options.security || {});
    },
    decode(sconString, options = {}) {
        const decoder = new Decoder(options);
        return decoder.decode(sconString);
    },
    minify(sconString) { return Minifier.minify(sconString); },
    expand(minifiedString, options = {}) { return Minifier.expand(minifiedString, options.indent ?? 1); },
};

// WASM module — zero-intermediate Rust tape decoder compiled to WebAssembly
let WASM = null;
try {
    const wasmDir = join(__dirname, '..', 'wasm', 'pkg');
    const wasmMod = await import(join(wasmDir, 'scon_wasm.js'));
    const wasmBytes = readFileSync(join(wasmDir, 'scon_wasm_bg.wasm'));
    await wasmMod.default(wasmBytes);
    WASM = {
        encode: wasmMod.scon_encode,
        decode: wasmMod.scon_decode,
        decodeViaJson: (s) => JSON.parse(wasmMod.scon_to_json(s)),
        minify: wasmMod.scon_minify,
        expand: wasmMod.scon_expand,
    };
    console.log('  WASM module loaded: wasm/pkg/scon_wasm_bg.wasm');
} catch (e) {
    console.log('  WASM module not available:', e.message);
}

// ============================================================================
// CLI args
// ============================================================================
const args = process.argv.slice(2);
const iterFlag = args.find(a => a.startsWith('--iterations='));
const ITERATIONS = iterFlag ? parseInt(iterFlag.split('=')[1]) : 100;

console.log('╔══════════════════════════════════════════════════════════╗');
console.log(`║  SCON Benchmark Suite — Node.js ${process.version.padEnd(22)}║`);
console.log(`║  Iterations: ${String(ITERATIONS).padEnd(43)}║`);
console.log('╚══════════════════════════════════════════════════════════╝\n');

// ============================================================================
// 1. LOAD CANONICAL FIXTURES (byte-identical across PHP, JS, Rust)
// ============================================================================

function loadFixtures() {
    const fixtureDir = join(__dirname, 'fixtures');
    const slugToName = {
        'openapi_specs':  'OpenAPI Specs',
        'config_records': 'Config Records',
        'db_exports':     'DB Exports',
    };
    const datasets = {};
    for (const [slug, name] of Object.entries(slugToName)) {
        const path = join(fixtureDir, `${slug}.json`);
        datasets[name] = JSON.parse(readFileSync(path, 'utf8'));
    }
    return datasets;
}

// ============================================================================
// 2. BENCHMARK FUNCTIONS
// ============================================================================

function formatBytes(bytes) {
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(2) + ' MB';
    if (bytes >= 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return bytes.toLocaleString() + ' B';
}

function pctChange(baseline, value) {
    if (baseline === 0) return 'N/A';
    const pct = ((value - baseline) / baseline) * 100;
    return (pct < 0 ? '' : '+') + pct.toFixed(1) + '%';
}

function benchmarkTiming(fn, iterations) {
    // Warmup
    for (let i = 0; i < Math.min(5, iterations); i++) fn();

    const times = [];
    for (let i = 0; i < iterations; i++) {
        const start = process.hrtime.bigint();
        fn();
        const elapsed = Number(process.hrtime.bigint() - start) / 1e6; // ms
        times.push(elapsed);
    }
    times.sort((a, b) => a - b);
    const n = times.length;
    const total = times.reduce((a, b) => a + b, 0);
    return {
        min: times[0],
        max: times[n - 1],
        mean: total / n,
        median: times[Math.floor(n * 0.5)],
        p95: times[Math.floor(n * 0.95)],
        p99: times[Math.floor(n * 0.99)],
        total,
        ops_per_sec: n / (total / 1000),
    };
}

async function benchmarkTimingAsync(fn, iterations) {
    // Warmup
    for (let i = 0; i < Math.min(5, iterations); i++) await fn();

    const times = [];
    for (let i = 0; i < iterations; i++) {
        const start = process.hrtime.bigint();
        await fn();
        const elapsed = Number(process.hrtime.bigint() - start) / 1e6;
        times.push(elapsed);
    }
    times.sort((a, b) => a - b);
    const n = times.length;
    const total = times.reduce((a, b) => a + b, 0);
    return {
        min: times[0],
        max: times[n - 1],
        mean: total / n,
        median: times[Math.floor(n * 0.5)],
        p95: times[Math.floor(n * 0.95)],
        p99: times[Math.floor(n * 0.99)],
        total,
        ops_per_sec: n / (total / 1000),
    };
}

function benchmarkMemory(fn) {
    // Warm
    fn();
    if (global.gc) global.gc();
    const before = process.memoryUsage().heapUsed;
    const result = fn();
    const after = process.memoryUsage().heapUsed;
    // Hold reference to prevent GC before measurement
    void result;
    return Math.max(0, after - before);
}

// ============================================================================
// 3. RUN BENCHMARKS
// ============================================================================

console.log('Loading fixtures from bench/fixtures/...');
const datasets = loadFixtures();
for (const [name, data] of Object.entries(datasets)) {
    const jsonSize = JSON.stringify(data).length;
    const topKeys = Object.keys(data).length;
    console.log(`  ${name}: ${jsonSize.toLocaleString()} bytes JSON (${topKeys} top-level keys)`);
}
console.log();

const results = [];

for (const [datasetName, data] of Object.entries(datasets)) {
    console.log(`━━━ ${datasetName} ━━━`);
    const r = { dataset: datasetName };

    // --- Payload Size ---
    // Use Buffer.byteLength for byte-accurate measurement (matches PHP strlen)
    const jsonStr = JSON.stringify(data);
    const jsonSize = Buffer.byteLength(jsonStr, 'utf8');
    const jsonGzipSize = gzipSync(jsonStr, { level: 9 }).length;

    const jsonPrettyStr = JSON.stringify(data, null, 2);
    const jsonPrettySize = Buffer.byteLength(jsonPrettyStr, 'utf8');
    const jsonPrettyGzipSize = gzipSync(jsonPrettyStr, { level: 9 }).length;

    const sconStr = SCON.encode(data);
    const sconSize = Buffer.byteLength(sconStr, 'utf8');
    const sconGzipSize = gzipSync(sconStr, { level: 9 }).length;

    const sconMinStr = SCON.minify(sconStr);
    const sconMinSize = Buffer.byteLength(sconMinStr, 'utf8');
    const sconMinGzipSize = gzipSync(sconMinStr, { level: 9 }).length;

    const sconDedupStr = await SCON.encode(data, { autoExtract: true });
    const sconDedupSize = Buffer.byteLength(sconDedupStr, 'utf8');
    const sconDedupGzipSize = gzipSync(sconDedupStr, { level: 9 }).length;

    const sconDedupMinStr = SCON.minify(sconDedupStr);
    const sconDedupMinSize = Buffer.byteLength(sconDedupMinStr, 'utf8');

    r.payload = {
        json: { raw: jsonSize, gzip: jsonGzipSize },
        json_pretty: { raw: jsonPrettySize, gzip: jsonPrettyGzipSize },
        scon: { raw: sconSize, gzip: sconGzipSize },
        scon_min: { raw: sconMinSize, gzip: sconMinGzipSize },
        scon_dedup: { raw: sconDedupSize, gzip: sconDedupGzipSize },
        scon_dedup_min: { raw: sconDedupMinSize },
    };

    console.log('  Payload Size:');
    console.log(`    JSON:             ${formatBytes(jsonSize)} (gzip: ${formatBytes(jsonGzipSize)})`);
    console.log(`    JSON(pretty):     ${formatBytes(jsonPrettySize)} (gzip: ${formatBytes(jsonPrettyGzipSize)})`);
    console.log(`    SCON:             ${formatBytes(sconSize)} (${pctChange(jsonSize, sconSize)}) (gzip: ${formatBytes(sconGzipSize)})`);
    console.log(`    SCON(min):        ${formatBytes(sconMinSize)} (${pctChange(jsonSize, sconMinSize)}) (gzip: ${formatBytes(sconMinGzipSize)})`);
    console.log(`    SCON(dedup):      ${formatBytes(sconDedupSize)} (${pctChange(jsonSize, sconDedupSize)})`);
    console.log(`    SCON(dedup+min):  ${formatBytes(sconDedupMinSize)} (${pctChange(jsonSize, sconDedupMinSize)})`);

    const dedupReduction = sconSize > 0 ? ((sconSize - sconDedupSize) / sconSize * 100) : 0;
    r.dedup_ratio = { scon_plain: sconSize, scon_dedup: sconDedupSize, reduction_pct: +dedupReduction.toFixed(1) };
    console.log(`    Dedup reduction:  ${dedupReduction.toFixed(1)}% additional savings`);

    // --- Encoding Time ---
    console.log(`  Encoding Time (${ITERATIONS} iterations):`);

    const jsonEnc = benchmarkTiming(() => JSON.stringify(data), ITERATIONS);
    console.log(`    JSON.stringify:   ${jsonEnc.median.toFixed(3)}ms (p95: ${jsonEnc.p95.toFixed(3)}ms, p99: ${jsonEnc.p99.toFixed(3)}ms) — ${Math.round(jsonEnc.ops_per_sec)} ops/s`);

    const sconEnc = benchmarkTiming(() => SCON.encode(data), ITERATIONS);
    console.log(`    SCON.encode:      ${sconEnc.median.toFixed(3)}ms (p95: ${sconEnc.p95.toFixed(3)}ms, p99: ${sconEnc.p99.toFixed(3)}ms) — ${Math.round(sconEnc.ops_per_sec)} ops/s`);

    const sconDedupEnc = await benchmarkTimingAsync(async () => SCON.encode(data, { autoExtract: true }), ITERATIONS);
    console.log(`    SCON(dedup):      ${sconDedupEnc.median.toFixed(3)}ms (p95: ${sconDedupEnc.p95.toFixed(3)}ms, p99: ${sconDedupEnc.p99.toFixed(3)}ms) — ${Math.round(sconDedupEnc.ops_per_sec)} ops/s`);

    r.encode = { json: jsonEnc, scon: sconEnc, scon_dedup: sconDedupEnc };

    // --- WASM Encoding ---
    if (WASM) {
        const wasmEnc = benchmarkTiming(() => WASM.encode(data), ITERATIONS);
        console.log(`    WASM.encode:      ${wasmEnc.median.toFixed(3)}ms (p95: ${wasmEnc.p95.toFixed(3)}ms, p99: ${wasmEnc.p99.toFixed(3)}ms) — ${Math.round(wasmEnc.ops_per_sec)} ops/s`);
        r.encode.wasm = wasmEnc;
    }

    // --- Decoding Time ---
    console.log(`  Decoding Time (${ITERATIONS} iterations):`);

    const jsonDec = benchmarkTiming(() => JSON.parse(jsonStr), ITERATIONS);
    console.log(`    JSON.parse:       ${jsonDec.median.toFixed(3)}ms (p95: ${jsonDec.p95.toFixed(3)}ms, p99: ${jsonDec.p99.toFixed(3)}ms) — ${Math.round(jsonDec.ops_per_sec)} ops/s`);

    const sconDec = benchmarkTiming(() => SCON.decode(sconStr), ITERATIONS);
    console.log(`    SCON.decode:      ${sconDec.median.toFixed(3)}ms (p95: ${sconDec.p95.toFixed(3)}ms, p99: ${sconDec.p99.toFixed(3)}ms) — ${Math.round(sconDec.ops_per_sec)} ops/s`);

    const sconMinDec = benchmarkTiming(() => SCON.decode(sconMinStr), ITERATIONS);
    console.log(`    SCON(min)decode:  ${sconMinDec.median.toFixed(3)}ms (p95: ${sconMinDec.p95.toFixed(3)}ms, p99: ${sconMinDec.p99.toFixed(3)}ms) — ${Math.round(sconMinDec.ops_per_sec)} ops/s`);

    r.decode = { json: jsonDec, scon: sconDec, scon_min: sconMinDec };

    // --- WASM Decoding ---
    if (WASM) {
        const wasmDec = benchmarkTiming(() => WASM.decode(sconStr), ITERATIONS);
        console.log(`    WASM.decode:      ${wasmDec.median.toFixed(3)}ms (p95: ${wasmDec.p95.toFixed(3)}ms, p99: ${wasmDec.p99.toFixed(3)}ms) — ${Math.round(wasmDec.ops_per_sec)} ops/s`);
        r.decode.wasm = wasmDec;

        const wasmV2Dec = benchmarkTiming(() => WASM.decodeViaJson(sconStr), ITERATIONS);
        console.log(`    WASM→JSON.parse:  ${wasmV2Dec.median.toFixed(3)}ms (p95: ${wasmV2Dec.p95.toFixed(3)}ms, p99: ${wasmV2Dec.p99.toFixed(3)}ms) — ${Math.round(wasmV2Dec.ops_per_sec)} ops/s`);
        r.decode.wasm_v2 = wasmV2Dec;
    }

    // --- Minify/Expand ---
    console.log(`  Minify/Expand (${ITERATIONS} iterations):`);

    const minBench = benchmarkTiming(() => SCON.minify(sconStr), ITERATIONS);
    console.log(`    minify(JS):       ${minBench.median.toFixed(3)}ms — ${Math.round(minBench.ops_per_sec)} ops/s`);

    const expBench = benchmarkTiming(() => SCON.expand(sconMinStr), ITERATIONS);
    console.log(`    expand(JS):       ${expBench.median.toFixed(3)}ms — ${Math.round(expBench.ops_per_sec)} ops/s`);

    if (WASM) {
        const wasmMinBench = benchmarkTiming(() => WASM.minify(sconStr), ITERATIONS);
        console.log(`    minify(WASM):     ${wasmMinBench.median.toFixed(3)}ms — ${Math.round(wasmMinBench.ops_per_sec)} ops/s`);
        const wasmExpBench = benchmarkTiming(() => WASM.expand(sconMinStr, 1), ITERATIONS);
        console.log(`    expand(WASM):     ${wasmExpBench.median.toFixed(3)}ms — ${Math.round(wasmExpBench.ops_per_sec)} ops/s`);
        r.minify_expand_wasm = { minify: wasmMinBench, expand: wasmExpBench };
    }

    r.minify_expand = {
        minify: minBench,
        expand: expBench,
        size_savings_pct: +((sconSize - sconMinSize) / sconSize * 100).toFixed(1),
    };

    // --- Memory ---
    console.log('  Memory (heap delta):');

    const jsonDecMem = benchmarkMemory(() => JSON.parse(jsonStr));
    console.log(`    JSON.parse:       ${formatBytes(jsonDecMem)}`);

    const sconDecMem = benchmarkMemory(() => SCON.decode(sconStr));
    console.log(`    SCON.decode:      ${formatBytes(sconDecMem)}`);

    const jsonEncMem = benchmarkMemory(() => JSON.stringify(data));
    console.log(`    JSON.stringify:   ${formatBytes(jsonEncMem)}`);

    const sconEncMem = benchmarkMemory(() => SCON.encode(data));
    console.log(`    SCON.encode:      ${formatBytes(sconEncMem)}`);

    r.memory = { json_decode: jsonDecMem, scon_decode: sconDecMem, json_encode: jsonEncMem, scon_encode: sconEncMem };

    // --- Throughput (MB/s) ---
    const jsonDecMBs = (jsonSize / 1048576) * jsonDec.ops_per_sec;
    const sconDecMBs = (sconSize / 1048576) * sconDec.ops_per_sec;
    const jsonEncMBs = (jsonSize / 1048576) * jsonEnc.ops_per_sec;
    const sconEncMBs = (sconSize / 1048576) * sconEnc.ops_per_sec;

    console.log('  Throughput:');
    console.log(`    JSON.parse:       ${jsonDecMBs.toFixed(2)} MB/s`);
    console.log(`    SCON.decode:      ${sconDecMBs.toFixed(2)} MB/s`);
    console.log(`    JSON.stringify:   ${jsonEncMBs.toFixed(2)} MB/s`);
    console.log(`    SCON.encode:      ${sconEncMBs.toFixed(2)} MB/s`);

    r.throughput = {
        json_decode_mbs: +jsonDecMBs.toFixed(2),
        scon_decode_mbs: +sconDecMBs.toFixed(2),
        json_encode_mbs: +jsonEncMBs.toFixed(2),
        scon_encode_mbs: +sconEncMBs.toFixed(2),
    };

    // --- Roundtrip verification ---
    const decoded = SCON.decode(sconStr);
    const reEncoded = SCON.encode(decoded);
    const roundtripOk = reEncoded === sconStr;
    console.log(`  Roundtrip:          ${roundtripOk ? 'OK' : 'FAIL'}`);

    const minDecoded = SCON.decode(sconMinStr);
    const minReEncoded = SCON.encode(minDecoded);
    const minRoundtripData = JSON.stringify(minDecoded) === JSON.stringify(data);
    console.log(`  Min roundtrip:      ${minRoundtripData ? 'OK (data)' : 'FAIL'}`);

    if (WASM) {
        const wasmDecoded = WASM.decode(sconStr);
        const wasmRt = JSON.stringify(wasmDecoded) === JSON.stringify(data);
        console.log(`  WASM roundtrip:     ${wasmRt ? 'OK' : 'FAIL'}`);
    }

    results.push(r);
    console.log();
}

// ============================================================================
// 4. SUMMARY TABLES
// ============================================================================

console.log('╔══════════════════════════════════════════════════════════════════════════════╗');
console.log('║  SUMMARY TABLES                                                            ║');
console.log('╚══════════════════════════════════════════════════════════════════════════════╝\n');

const pad = (s, n, right = false) => right ? String(s).padStart(n) : String(s).padEnd(n);

console.log('Payload Size (Bytes):');
console.log(pad('Dataset', 20) + pad('JSON', 12, true) + pad('SCON', 12, true)
    + pad('SCON(min)', 12, true) + pad('JSON+Gz', 12, true)
    + pad('SCON+Gz', 12, true) + pad('Saving', 10, true));
console.log('─'.repeat(90));
for (const r of results) {
    const p = r.payload;
    const saving = ((1 - p.scon.raw / p.json.raw) * 100).toFixed(0) + '%';
    console.log(pad(r.dataset, 20) + pad(p.json.raw.toLocaleString(), 12, true)
        + pad(p.scon.raw.toLocaleString(), 12, true) + pad(p.scon_min.raw.toLocaleString(), 12, true)
        + pad(p.json.gzip.toLocaleString(), 12, true) + pad(p.scon.gzip.toLocaleString(), 12, true)
        + pad(saving, 10, true));
}

console.log('\nDecode Time — median ms:');
console.log(pad('Dataset', 18) + pad('JSON.parse', 12, true) + pad('SCON(JS)', 12, true) + pad('WASM(v1)', 12, true) + pad('WASM(v2)', 12, true) + pad('JS/JSON', 9, true) + pad('v2/JSON', 9, true));
console.log('─'.repeat(84));
for (const r of results) {
    const jd = r.decode.json;
    const sd = r.decode.scon;
    const wd = r.decode.wasm;
    const w2 = r.decode.wasm_v2;
    const jsRatio = jd.median > 0 ? (sd.median / jd.median).toFixed(1) + 'x' : 'N/A';
    const v2Ratio = w2 && jd.median > 0 ? (w2.median / jd.median).toFixed(1) + 'x' : 'N/A';
    console.log(pad(r.dataset, 18)
        + pad(jd.median.toFixed(3), 12, true)
        + pad(sd.median.toFixed(3), 12, true)
        + pad(wd ? wd.median.toFixed(3) : '—', 12, true)
        + pad(w2 ? w2.median.toFixed(3) : '—', 12, true)
        + pad(jsRatio, 9, true)
        + pad(v2Ratio, 9, true));
}

console.log('\nEncode Time — median ms (JSON.stringify | SCON JS | SCON WASM):');
console.log(pad('Dataset', 20) + pad('JSON.stringify', 15, true) + pad('SCON(JS)', 15, true) + pad('SCON(WASM)', 15, true) + pad('JS/JSON', 10, true) + pad('WASM/JSON', 10, true));
console.log('─'.repeat(85));
for (const r of results) {
    const je = r.encode.json;
    const se = r.encode.scon;
    const we = r.encode.wasm;
    const jsRatio = je.median > 0 ? (se.median / je.median).toFixed(1) + 'x' : 'N/A';
    const wasmRatio = we && je.median > 0 ? (we.median / je.median).toFixed(1) + 'x' : 'N/A';
    console.log(pad(r.dataset, 20)
        + pad(je.median.toFixed(3), 15, true)
        + pad(se.median.toFixed(3), 15, true)
        + pad(we ? we.median.toFixed(3) : '—', 15, true)
        + pad(jsRatio, 10, true)
        + pad(wasmRatio, 10, true));
}

console.log('\nDeduplication (autoExtract):');
console.log(pad('Dataset', 20) + pad('SCON plain', 12, true) + pad('SCON dedup', 12, true) + pad('Reduction', 12, true));
console.log('─'.repeat(56));
for (const r of results) {
    const d = r.dedup_ratio;
    console.log(pad(r.dataset, 20)
        + pad(d.scon_plain.toLocaleString(), 12, true)
        + pad(d.scon_dedup.toLocaleString(), 12, true)
        + pad(d.reduction_pct + '%', 12, true));
}

console.log('\nMemory (heap delta):');
console.log(pad('Dataset', 20) + pad('json_dec', 12, true) + pad('scon_dec', 12, true)
    + pad('json_enc', 12, true) + pad('scon_enc', 12, true));
console.log('─'.repeat(68));
for (const r of results) {
    const m = r.memory;
    console.log(pad(r.dataset, 20)
        + pad(formatBytes(m.json_decode), 12, true) + pad(formatBytes(m.scon_decode), 12, true)
        + pad(formatBytes(m.json_encode), 12, true) + pad(formatBytes(m.scon_encode), 12, true));
}

console.log('\nThroughput (MB/s):');
console.log(pad('Dataset', 20) + pad('json_dec', 12, true) + pad('scon_dec', 12, true)
    + pad('json_enc', 12, true) + pad('scon_enc', 12, true));
console.log('─'.repeat(68));
for (const r of results) {
    const t = r.throughput;
    console.log(pad(r.dataset, 20)
        + pad(t.json_decode_mbs.toFixed(2), 12, true) + pad(t.scon_decode_mbs.toFixed(2), 12, true)
        + pad(t.json_encode_mbs.toFixed(2), 12, true) + pad(t.scon_encode_mbs.toFixed(2), 12, true));
}

// ============================================================================
// 5. JSON output — incremental
// ============================================================================

const outDir = join(__dirname, 'datasets');
if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });

const now = new Date();
const ts = now.toISOString().replace(/[-:T]/g, '').slice(0, 14).replace(/(\d{8})(\d{6})/, '$1_$2');
const outPath = join(outDir, `js_${ts}.json`);

writeFileSync(outPath, JSON.stringify({
    meta: {
        lang: 'js',
        suite: 'standard',
        fixture_source: 'bench/fixtures/',
        node_version: process.version,
        v8_version: process.versions.v8,
        iterations: ITERATIONS,
        date: now.toISOString(),
        timestamp: Math.floor(now.getTime() / 1000),
        hostname: (await import('os')).hostname(),
    },
    results,
}, null, 2));

console.log(`\nJSON results saved to: ${outPath}`);
console.log('Done.');
