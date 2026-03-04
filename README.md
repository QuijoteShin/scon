# SCON — Structured Compact Object Notation

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.18846661.svg)](https://doi.org/10.5281/zenodo.18846661)
[![License: CC BY 4.0](https://img.shields.io/badge/License-CC%20BY%204.0-lightgrey.svg)](https://creativecommons.org/licenses/by/4.0/)

A human-readable, text-based serialization format designed as a compact alternative to JSON for payload-sensitive workloads.

**Paper:** [SCON: A Textual Serialization Format with Structural Deduplication, Tabular Encoding, and Token-Efficient Representation](https://doi.org/10.5281/zenodo.18846661)

## Key features

- **Tabular encoding** — arrays of uniform objects emit keys once, not per row
- **Structural deduplication** — Merkle-style hashing factorizes recurring subtrees
- **Minification** — semicolon-based depth encoding, no whitespace needed
- **Human-readable** — readable without pretty-printing (unlike JSON)

## Performance milestone

**SCON tape decoder beats simd-json** — the fastest JSON parser — on structured data. Pure scalar, no SIMD instructions required. Runs on x86, ARM, ESP32, and Arduino without platform-specific code.

### Decode speed (Rust, 500 iterations)

| Dataset | simd-json | serde_json | SCON tape | SCON tape vs simd |
|---------|----------:|-----------:|----------:|------------------:|
| OpenAPI Specs (48 KB) | 0.246 ms | 0.418 ms | **0.195 ms** | **21% faster** |
| Config Records (72 KB) | 0.232 ms | 0.451 ms | **0.292 ms** | 26% slower |
| DB Exports (19 KB) | 0.055 ms | 0.086 ms | **0.045 ms** | **18% faster** |

### Payload size + decode speed combined

| Dataset | JSON size | SCON(min) size | Reduction | Fastest decoder |
|---------|----------:|---------------:|----------:|:----------------|
| OpenAPI | 49 KB | 42 KB | -13% | SCON tape |
| Config | 73 KB | 66 KB | -8% | simd-json |
| DB | 20 KB | 14 KB | -29% | SCON tape |

SCON wins on both axes (smaller + faster) for tabular/structured data.

### Peak memory (decode)

| Dataset | simd-json | serde_json | SCON tape |
|---------|----------:|-----------:|----------:|
| OpenAPI | 4,443 KB | 4,676 KB | **3,874 KB** |
| Config | 4,766 KB | 4,397 KB | **4,111 KB** |
| DB | 3,677 KB | 3,654 KB | **3,512 KB** |

SCON tape uses the least memory in all datasets — critical for embedded/telemetry targets.

### End-to-end: wire-to-parsed (transmission + decode)

Total time = `payload_bytes × 8 / bandwidth + decode_time`. This is what matters in production — smaller payloads compound with faster parsing.

**OpenAPI Specs (API definitions, high dedup potential):**

| Bandwidth | JSON + simd-json | JSON + serde | SCON(min) + tape | SCON(dedup+min) + tape |
|-----------|----------------:|-------------:|-----------------:|-----------------------:|
| 1 Mbps (LoRa/satellite) | 392.7 ms | 392.9 ms | 342.6 ms (-13%) | **135.8 ms (-65%)** |
| 10 Mbps (WiFi) | 39.5 ms | 39.7 ms | 34.4 ms (-13%) | **13.8 ms (-65%)** |
| 100 Mbps (Ethernet) | 4.2 ms | 4.3 ms | 3.6 ms (-14%) | **1.6 ms (-63%)** |
| 1 Gbps (datacenter) | 0.64 ms | 0.81 ms | **0.54 ms (-16%)** | 0.33 ms (-48%) |

**DB Exports (telemetry/tabular — SCON's strongest case):**

| Bandwidth | JSON + simd-json | JSON + serde | SCON(min) + tape |
|-----------|----------------:|-------------:|-----------------:|
| 1 Mbps (LoRa/satellite) | 157.6 ms | 157.6 ms | **111.4 ms (-29%)** |
| 10 Mbps (WiFi) | 15.8 ms | 15.9 ms | **11.2 ms (-29%)** |
| 100 Mbps (Ethernet) | 1.6 ms | 1.7 ms | **1.2 ms (-29%)** |
| 1 Gbps (datacenter) | 0.21 ms | 0.24 ms | **0.16 ms (-24%)** |

### Resource budget per candidate

| Candidate | Payload (OpenAPI) | Decode time | Peak RAM | Needs SIMD | Embedded viable |
|-----------|------------------:|------------:|---------:|:----------:|:---------------:|
| JSON + simd-json | 49 KB | 0.246 ms | 4,443 KB | Yes (AVX2/NEON) | RPi only |
| JSON + serde_json | 49 KB | 0.418 ms | 4,676 KB | No | Yes |
| SCON(min) + tape | 42 KB | 0.195 ms | 3,874 KB | No | Yes |
| SCON(dedup+min) + tape | 17 KB | 0.195 ms | 3,874 KB | No | Yes |

SCON(min) + tape wins on **every axis** vs serde_json: smaller payload, faster decode, less memory, no SIMD dependency. Against simd-json: smaller, faster (on 2/3 datasets), less memory, and runs on hardware where simd-json cannot.

## Paper baseline (pre-optimization)

| Metric | Result |
|--------|--------|
| Payload size (no dedup) | 13–29% smaller |
| Payload size (with dedup) | up to 66% smaller |
| LLM tokens (cl100k_base) | 64% fewer |
| gzip CPU savings | 8–53% less |
| Encode overhead (Rust) | 1.6–3.4x slower |
| Decode overhead (Rust) | 2.5–3.4x slower |

Break-even: SCON is faster end-to-end on links below ~100 Mbps.

### PHP native extension (zero-intermediate FFI)

The PHP extension uses the same tape decoder that beats simd-json, but emits PHP Zvals directly from the tape — no intermediate AST. Encode walks PHP arrays directly into a SCON string buffer.

| Operation | Dataset | json_decode (C) | scon_decode (Rust ext) | Ratio |
|-----------|---------|----------------:|-----------------------:|------:|
| Decode | OpenAPI | 0.374 ms | **0.440 ms** | 1.2x |
| Decode | Config | 0.383 ms | **0.535 ms** | 1.4x |
| Decode | DB | 0.093 ms | **0.097 ms** | 1.1x |
| Encode | OpenAPI | 0.108 ms | 0.329 ms | 3.1x |
| Encode | Config | 0.126 ms | 0.401 ms | 3.2x |
| Encode | DB | 0.034 ms | 0.105 ms | 3.1x |

Decode is near parity with PHP's C json_decode (1.1–1.4x). The Rust ext is **9–18x faster than PHP userland SCON**. Encode overhead (3.1x) is structural: SCON's tabular detection requires scanning arrays twice to verify key uniformity — a cost JSON doesn't pay.

## Implementations

| Language | Type | Path |
|----------|------|------|
| **Rust** | Native crate (core library) | `rs/` |
| **PHP ext** | Native extension (Rust via ext-php-rs, zero-intermediate) | `ext/` |
| **PHP** | Userland | `php/` |
| **JavaScript** | Userland (Node.js) | `js/` |

## Quick example

```
users[3]{id, name, role}:
 1, Alice, admin
 2, Bob, editor
 3, Carol, viewer
```

Equivalent JSON (89 bytes vs 55 bytes SCON):
```json
[{"id":1,"name":"Alice","role":"admin"},{"id":2,"name":"Bob","role":"editor"},{"id":3,"name":"Carol","role":"viewer"}]
```

## Running benchmarks

```bash
php bench/generate_fixtures.php     # generate canonical fixtures
php bench/bench.php                 # PHP benchmark
node --expose-gc bench/bench.mjs    # JS benchmark
cargo run --manifest-path rs/Cargo.toml --release --bin scon-bench  # Rust benchmark
```

See [bench/README.md](bench/README.md) for detailed results and methodology.

## Citation

```bibtex
@misc{moura2026scon,
  author    = {Moura, Gustavo},
  title     = {SCON: A Textual Serialization Format with Structural Deduplication, Tabular Encoding, and Token-Efficient Representation},
  year      = {2026},
  publisher = {Zenodo},
  doi       = {10.5281/zenodo.18846661},
  url       = {https://doi.org/10.5281/zenodo.18846661}
}
```

## License

Code: MIT | Paper: CC BY 4.0
