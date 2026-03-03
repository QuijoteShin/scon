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

    // --- Decoding Time ---
    console.log(`  Decoding Time (${ITERATIONS} iterations):`);

    const jsonDec = benchmarkTiming(() => JSON.parse(jsonStr), ITERATIONS);
    console.log(`    JSON.parse:       ${jsonDec.median.toFixed(3)}ms (p95: ${jsonDec.p95.toFixed(3)}ms, p99: ${jsonDec.p99.toFixed(3)}ms) — ${Math.round(jsonDec.ops_per_sec)} ops/s`);

    const sconDec = benchmarkTiming(() => SCON.decode(sconStr), ITERATIONS);
    console.log(`    SCON.decode:      ${sconDec.median.toFixed(3)}ms (p95: ${sconDec.p95.toFixed(3)}ms, p99: ${sconDec.p99.toFixed(3)}ms) — ${Math.round(sconDec.ops_per_sec)} ops/s`);

    const sconMinDec = benchmarkTiming(() => SCON.decode(sconMinStr), ITERATIONS);
    console.log(`    SCON(min)decode:  ${sconMinDec.median.toFixed(3)}ms (p95: ${sconMinDec.p95.toFixed(3)}ms, p99: ${sconMinDec.p99.toFixed(3)}ms) — ${Math.round(sconMinDec.ops_per_sec)} ops/s`);

    r.decode = { json: jsonDec, scon: sconDec, scon_min: sconMinDec };

    // --- Minify/Expand ---
    console.log(`  Minify/Expand (${ITERATIONS} iterations):`);

    const minBench = benchmarkTiming(() => SCON.minify(sconStr), ITERATIONS);
    console.log(`    minify:           ${minBench.median.toFixed(3)}ms — ${Math.round(minBench.ops_per_sec)} ops/s`);

    const expBench = benchmarkTiming(() => SCON.expand(sconMinStr), ITERATIONS);
    console.log(`    expand:           ${expBench.median.toFixed(3)}ms — ${Math.round(expBench.ops_per_sec)} ops/s`);

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

console.log('\nParsing Time — median ms (p95 / p99):');
console.log(pad('Dataset', 20) + pad('JSON.parse', 22, true) + pad('SCON.decode', 22, true) + pad('Ratio', 10, true));
console.log('─'.repeat(74));
for (const r of results) {
    const jd = r.decode.json;
    const sd = r.decode.scon;
    const ratio = jd.median > 0 ? (sd.median / jd.median).toFixed(1) + 'x' : 'N/A';
    console.log(pad(r.dataset, 20)
        + pad(`${jd.median.toFixed(2)} (${jd.p95.toFixed(2)}/${jd.p99.toFixed(2)})`, 22, true)
        + pad(`${sd.median.toFixed(2)} (${sd.p95.toFixed(2)}/${sd.p99.toFixed(2)})`, 22, true)
        + pad(ratio, 10, true));
}

console.log('\nEncoding Time — median ms (p95 / p99):');
console.log(pad('Dataset', 20) + pad('JSON.stringify', 22, true) + pad('SCON.encode', 22, true) + pad('Ratio', 10, true));
console.log('─'.repeat(74));
for (const r of results) {
    const je = r.encode.json;
    const se = r.encode.scon;
    const ratio = je.median > 0 ? (se.median / je.median).toFixed(1) + 'x' : 'N/A';
    console.log(pad(r.dataset, 20)
        + pad(`${je.median.toFixed(2)} (${je.p95.toFixed(2)}/${je.p99.toFixed(2)})`, 22, true)
        + pad(`${se.median.toFixed(2)} (${se.p95.toFixed(2)}/${se.p99.toFixed(2)})`, 22, true)
        + pad(ratio, 10, true));
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
