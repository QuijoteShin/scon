# bench/README.md
# SCON Benchmark Suite

Cross-language benchmark comparing SCON encode/decode performance against native JSON implementations.
All benchmarks read from canonical fixture files to ensure identical input data.

## Fixture generation

Fixtures are JSON files generated once by PHP (`srand(42)`) and committed to git.
All three language benchmarks parse these files — ensuring byte-identical input data.

```bash
# Generate/regenerate fixtures (requires PHP)
php bench/generate_fixtures.php
```

Output:

| Fixture | Size | Description |
|---------|-----:|-------------|
| `bench/fixtures/openapi_specs.json` | 49,067 B | 70 REST endpoints with parameters and response schemas |
| `bench/fixtures/config_records.json` | 73,183 B | 40 service configs + 200 feature flags |
| `bench/fixtures/db_exports.json` | 19,688 B | 24 table DDL schemas with columns and indexes |

Fixtures are committed to git. Regenerate only when the generation algorithm changes.

## Running benchmarks

```bash
# PHP — standard datasets
php bench/bench.php

# PHP — 10MB datasets (server logs, geo API, multimedia)
php bench/bench_10mb.php

# JavaScript (Node.js) — standard datasets
node --expose-gc bench/bench.mjs

# Rust — standard datasets (build + run)
cargo build --release && ./target/release/scon-bench
```

### Options

```bash
# PHP: custom iterations and output format
php bench/bench.php --iterations=200 --output=json|table|both

# PHP 10MB: custom iterations
php bench/bench_10mb.php --iterations=50

# JS: custom iterations
node --expose-gc bench/bench.mjs --iterations=200

# Rust: custom iterations
./target/release/scon-bench --iterations=200
```

## Results

Each run saves a JSON file to `bench/datasets/` (gitignored):

```
bench/datasets/{lang}_{timestamp}.json
```

All outputs share a standardized schema with `fixture_source`, payload sizes (including `json_pretty`), timing stats (min/max/mean/median/p95/p99/ops_per_sec), and throughput.

## What is being compared

| Language | JSON impl | SCON impl |
|----------|-----------|-----------|
| **PHP** | `json_encode`/`json_decode` (C extension) | Userland PHP |
| **JavaScript** | `JSON.stringify`/`JSON.parse` (V8 native) | Userland JS |
| **Rust** | `serde_json` (native Rust) | `scon-core` crate (native Rust) |

The Rust benchmark provides the fairest format-vs-format comparison since both sides are native compiled code. PHP and JS benchmarks show real-world performance where SCON runs in userland against C/C++ JSON engines.

## Payload comparison (4 variants)

All benchmarks measure these payload variants for the paper:

| Variant | Description |
|---------|-------------|
| `json` | JSON minified (`json_encode` / `JSON.stringify` / `serde_json::to_string`) |
| `json_pretty` | JSON pretty-printed (2-space indent) |
| `scon` | SCON standard (1-space indent) |
| `scon_min` | SCON minified (no indentation) |

PHP and JS additionally measure `scon_dedup` and `scon_dedup_min` (structural deduplication via TreeHash). Rust does not implement dedup — those fields are omitted.

## Insights

### Payload size

| Dataset | JSON | JSON pretty | SCON | SCON(min) | SCON(dedup+min) |
|---------|-----:|------------:|-----:|----------:|----------------:|
| OpenAPI Specs | 49 KB | 119 KB | 57 KB | 42 KB | 17 KB |
| Config Records | 73 KB | 122 KB | 75 KB | 67 KB | 65 KB |
| DB Exports | 20 KB | 32 KB | 15 KB | 14 KB | 14 KB |

Key findings:
- **SCON(min) is always smaller than JSON(min)** — 13% smaller on OpenAPI, 8% on Config Records, 29% on DB Exports
- **SCON(dedup+min) achieves 66% reduction** on OpenAPI Specs vs JSON
- **JSON pretty-print is 2.5x larger** than JSON minified on OpenAPI — SCON's readable format adds only 18% overhead vs JSON min

### Speed: Rust native (format-vs-format)

The fairest comparison — both serde_json and SCON are compiled Rust:

| Dataset | serde_json enc | SCON enc | Ratio | serde_json dec | SCON dec (owned) | Ratio | SCON dec (borrowed) | Ratio |
|---------|---------------:|---------:|------:|---------------:|----------------:|------:|-------------------:|------:|
| OpenAPI Specs | 0.056 ms | 0.061 ms | 1.1x | 0.367 ms | 0.630 ms | 1.7x | 0.543 ms | **1.5x** |
| Config Records | 0.078 ms | 0.091 ms | 1.2x | 0.422 ms | 0.655 ms | 1.6x | 0.597 ms | **1.4x** |
| DB Exports | 0.021 ms | 0.041 ms | 1.9x | 0.085 ms | 0.149 ms | 1.8x | 0.130 ms | **1.5x** |

SCON encoding is near parity with serde_json (1.1x on OpenAPI). The **owned decoder** (1.6–1.8x) uses `CompactString` for all strings. The **borrowed decoder** (1.4–1.5x) returns `&str` slices borrowed directly from the input buffer — zero-copy for ~90% of strings (those without escape sequences). Escaped strings (~10%) are allocated in a `bumpalo` arena. The remaining 1.4x gap is architectural: two-pass line classification + IndexMap allocation vs serde_json's single-pass recursive descent.

### Paper publication baseline

The table above reflects optimizations completed post-publication. The pre-optimization baseline used for the paper is preserved in:

```
bench/datasets/rust_p0_baseline_20260303_195334.json
```

| Dataset | serde_json enc | SCON enc | Ratio | serde_json dec | SCON dec | Ratio |
|---------|---------------:|---------:|------:|---------------:|---------:|------:|
| OpenAPI Specs | 0.061 ms | 0.099 ms | 1.6x | 0.343 ms | 0.953 ms | 2.8x |
| Config Records | 0.087 ms | 0.187 ms | 2.2x | 0.457 ms | 1.130 ms | 2.5x |
| DB Exports | 0.022 ms | 0.075 ms | 3.4x | 0.091 ms | 0.312 ms | 3.4x |

Phase 3 benchmark: `bench/datasets/rust_p3_all_final_20260303_222358.json`
Phase 4 benchmark (scratch buffer + fast-path unescape): `bench/datasets/rust_p4_scratch_unescape_20260303_225941.json`
Phase 5 benchmark (no-rescan + memchr3 + inline split + bracket pre-filter): `bench/datasets/rust_p5_all_final_20260303_234453.json`
Phase 6 benchmark (CompactString keys + values): `bench/datasets/rust_p6_compact_keys_20260303_235715.json`
Phase 7 benchmark (borrowed zero-copy decoder): `bench/datasets/rust_p7_borrowed_zerocopy_20260304_003131.json`

### Post-publication optimization log

Each entry documents a change, its algorithmic impact, and measured result.

| # | Optimization | Complexity change | Measured impact |
|---|-------------|-------------------|-----------------|
| P1 | Zero-copy `ParsedLine` (borrows from input) | O(L) alloc → O(1) per line | Baseline improvement |
| P2 | `itoa`/`ryu` for numeric encoding | O(digits) format vs `write!` overhead | ~5% encode |
| P3 | Lookup tables `[256]` for byte classification | O(1) branch-free vs multi-compare | OpenAPI encode reached **1.0x** parity |
| P4 | `ArrayHeader<'a>` with borrowed slices | O(K) clone per header → O(0) | ~8% encode on tabular |
| P5 | `ahash` replacing SipHash in IndexMap | O(1) per hash but ~2x faster constant | ~10-24% encode/decode |
| P6 | `memchr` SIMD for delimiter search | O(L/16) SIMD vs O(L) scalar scan | ~5% decode |
| P7 | Fast-path unescape: `memchr(b'\\')` skip | O(1) check avoids O(L) byte-by-byte for ~90% of strings | OpenAPI decode **2.2x → 1.9x** |
| P8 | Scratch buffer for unescape (capacity reuse) | Amortized O(1) alloc vs O(N) fresh allocations | Combined with P7 |
| P9 | Manual integer parser (byte accumulator) | O(digits) unchecked vs stdlib `FromStr` + `Result` overhead | ~neutral on string-heavy data |
| P10 | Depth-skip elimination: child returns `next_index` | O(N) total vs O(N×D) re-scanning | ~5% decode, architecturally correct |
| P11 | `memchr3(b':', b'"', b'{')` single-pass colon search | O(L/16) single SIMD pass vs 3 sequential scans | ~neutral (lines are short), cleaner code |
| P12 | Inline `split_top_level` in `parse_delimited_values` | O(V) direct vs O(V) split + O(V) iterate (eliminates `Vec<&str>` alloc) | ~5% decode on Config/DB |
| P13 | `has_bracket` pre-filter on `ParsedLine` | O(1) bool check skips O(L) `try_array_header` for ~95% of lines | ~neutral — evita trabajo innecesario en hot loop |
| P14 | Chunk-based unescape via `memchr(b'\\')` | O(C) chunks vs O(L) byte-by-byte (C = escape count, C ≪ L) | ~neutral — fast-path already covers ~90% of strings |
| P15 | `CompactString` (inline ≤24 bytes, no heap alloc for keys/values) | O(1) inline vs O(L) heap alloc for ~90% of strings (keys average ~8 bytes) | Decode **1.9x → 1.6x** OpenAPI, **1.8x** DB |
| P16 | `BorrowedDecoder` — zero-copy `&'a str` from input + bumpalo arena | O(0) per string (borrow) vs O(L) copy to CompactString | Decode **1.7x → 1.5x** OpenAPI, **1.4x** Config |

**Complexity note on P10:** Before the fix, when `decode_object` called a child recursively, the child processed N lines, then the parent re-scanned the same N lines to find the next sibling (`while depth > base_depth { i++ }`). At depth D, the same line could be scanned D times — O(N×D) total. With the fix, each line is visited exactly once — O(N).

**Complexity note on P11:** `find_key_colon` previously did `memchr(b':')` then `prefix.contains(b'"')` + `prefix.contains(b'{')` — three O(L) passes over the same memory. `memchr3` finds the first occurrence of any of the three bytes in a single SIMD pass — O(L/16) with 128-bit vectors. Impact is minimal because SCON lines average ~30 bytes (below SIMD amortization threshold), but eliminates redundant memory reads.

### Key takeaways

1. **SCON's strength is payload size, not speed.** On tabular data, SCON(min) is 29% smaller than JSON without compression. With dedup, up to 66% smaller.

2. **The speed gap reflects implementation maturity.** Encoding has reached parity (1.0x on OpenAPI). The 2.2x decode ratio is architectural: SCON uses two-pass parsing (line classification + semantic interpretation) vs serde_json's single-pass recursive descent with manual number parsing and scratch buffer reuse.

3. **SCON is readable AND smaller.** JSON needs pretty-print to be human-readable (3.8x size increase). SCON's standard format is readable with only 17% overhead vs JSON minified.

4. **Network-bound workloads favor SCON.** When transmission latency dominates, smaller payloads matter more than microseconds of parsing.

5. **gzip equalizes size but not CPU.** After gzip both formats converge, but SCON starts from a smaller input — less CPU spent on compression.

### Note on cross-language JSON sizes

Each language's `json_encode` may produce slightly different byte counts from the same parsed data due to key ordering (serde_json sorts alphabetically, PHP preserves insertion order). The input data is identical (same fixture file); the `json.raw` field reflects each language's native serialization. This is intentional — the benchmark measures real-world performance of each ecosystem.

## Algorithmic complexity

Notation: **N** = total nodes in data tree, **L** = serialized string length, **D** = max nesting depth, **K** = max keys per object, **R** = rows in tabular array, **S** = registered schemas, **C** = changed nodes (for diff).

### Core operations

| Component | Time | Space | Algorithm |
|-----------|------|-------|-----------|
| **Encoder** | O(N) | O(D + L) | Single-pass recursive DFS. Tabular detection scans R×K to verify uniform keys before compact output. |
| **Decoder** | O(L) | O(N + D) | Two-pass: (1) line classification O(L), (2) body parse with monotonic pointer — each line visited at most twice. |
| **Minifier** | O(L) | O(L) | Streaming single-pass. Depth encoded as unary semicolons: `n` semicolons = dedent by `n-1` levels. |
| **Expand** | O(L) | O(L) | Character-by-character state machine (normal / in-quotes / counting-semicolons). |

### Tabular encoding (SCON's key optimization)

When the encoder detects an array of uniform objects (all items share the same keys with primitive values), it emits a single header line followed by R data rows:

```
tableName[R]{key1,key2,...}:
 val1, val2, ...
 val1, val2, ...
```

- **Detection**: O(R × K) — scan all rows to verify key uniformity
- **Output**: O(R × K) — one row per item, no repeated keys
- **Savings**: eliminates R×K key repetitions, which is why DB Exports shrinks 29% vs JSON

This is the fundamental size advantage over JSON, where every object repeats `{"key1":..., "key2":..., "key3":...}`.

### TreeHash — structural deduplication (xxHash128)

TreeHash uses [xxHash128](https://github.com/Cyan4973/xxHash) (XXH3 family) for structural deduplication. xxHash is a non-cryptographic hash optimized for speed (>50 GB/s on modern CPUs), originally designed for checksums, data integrity, and hash tables.

**Why xxHash128 (not SHA/MD5/CRC)**:
- **Speed**: 10-100x faster than cryptographic hashes — critical when hashing every subtree
- **128-bit width**: collision probability ~2⁻⁶⁴ for birthday-bound, sufficient for structural comparison (not security)
- **Streaming API**: can hash incrementally without buffering the full input
- **Deterministic cross-platform**: same input always produces the same hash regardless of OS/arch

**Two hashing strategies**:

| Strategy | Purpose | Method | Time |
|----------|---------|--------|------|
| **hashTree** (hybrid) | Dedup index | `json_encode` subtree → xxh128 | O(N × D) |
| **fingerprint** (Merkle) | Equality/diff | Bottom-up: `type:N\|fp₁\|fp₂\|...` → xxh128 | O(N) |

**hashTree** (used for `autoExtract` dedup):
1. `recursiveKsort` — normalize key order: O(N × K × log K)
2. `collectHashesHybrid` — DFS, at each object node: serialize subtree with `json_encode` → hash with xxh128 → store in index (hash map). A subtree of size M is serialized in O(M); across all nodes the total work is O(N × D) due to ancestor re-serialization.
3. Identical hashes → identical structures → extract as named schema (`@s:name`), replace occurrences with references.

**fingerprint** (Merkle tree — used for diff/equals):
1. Primitives return raw type-tagged bytes — no hash call
2. Arrays/objects concatenate children's fingerprints, then call xxh128 once per container
3. Total xxh128 calls = number of container nodes (not N)
4. **Merkle property**: `diff()` prunes identical subtrees in O(C × K) instead of O(N)

**Implementation by language**:

| Language | xxHash source | Latency per call |
|----------|---------------|------------------|
| PHP | `hash('xxh128', ...)` — native C extension (PHP 8.1+) | ~ns |
| JS | `hash-wasm` — compiled WASM, lazy-init singleton | ~ns after init |
| Rust | `xxhash-rust` crate (declared, not yet used — TreeHash not ported) | — |

### SchemaRegistry

| Operation | Time | Notes |
|-----------|------|-------|
| `register(name, schema)` | O(1) | Hash map insert |
| `resolve(name)` | O(schema_N) | O(1) lookup + deep reference resolution |
| `resolveWithOverride` | O(schema_N + override_N) | Base resolve + recursive merge |
| `findMatchingSchema` (encoder) | O(S × K) per object node | Deep equality vs each registered schema |

### Full pipeline complexity

| Pipeline | Time | Space |
|----------|------|-------|
| `encode(data)` | O(N) | O(D + L) |
| `encode(data, autoExtract)` | O(N × D) | O(N + L) |
| `decode(scon)` | O(L) | O(N + D) |
| `minify(scon)` | O(L) | O(L) |
| `expand(minified)` | O(L) | O(L) |
| `encode → minify` | O(N + L) | O(L) |
| `expand → decode` | O(L) | O(N + L) |
| `diff(tree_a, tree_b)` | O(C × K) | O(D) |

### Data structures

| Structure | PHP | JS | Rust | Purpose |
|-----------|-----|-----|------|---------|
| Key-value store | associative array | plain object | `IndexMap` | O(1) lookup, insertion-order |
| Schema store | associative array | plain object | — | O(1) name → schema |
| Hash index (dedup) | assoc array by hex | object by hex | — | xxh128 → subtree mapping |
| Value tree | mixed arrays | nested objects | `enum Value` | Recursive data model |
| Line buffer (decoder) | `$parsedLines[]` | `parsedLines[]` | `Vec<ParsedLine>` | Two-pass parse |

`IndexMap` (Rust, from the `indexmap` crate) provides O(1) average-case lookup with preserved insertion order — a hash map backed by an auxiliary `Vec`. This is important for round-trip fidelity: keys come out in the same order they went in.
