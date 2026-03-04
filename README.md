# SCON — Structured Compact Object Notation

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.18846661.svg)](https://doi.org/10.5281/zenodo.18846661)
[![License: CC BY 4.0](https://img.shields.io/badge/License-CC%20BY%204.0-lightgrey.svg)](https://creativecommons.org/licenses/by/4.0/)

A human-readable, text-based serialization format designed as a compact alternative to JSON for payload-sensitive workloads.

**Paper:** [SCON: A Textual Serialization Format with Structural Deduplication, Tabular Encoding, and Token-Efficient Representation](https://doi.org/10.5281/zenodo.18846661)

## What it looks like

```
users[3]{id, name, role}:
 1, Alice, admin
 2, Bob, editor
 3, Carol, viewer
```

Equivalent JSON — 89 bytes vs 55 bytes SCON:
```json
[{"id":1,"name":"Alice","role":"admin"},{"id":2,"name":"Bob","role":"editor"},{"id":3,"name":"Carol","role":"viewer"}]
```

Keys emitted once in the header, not repeated per row. This is SCON's core size advantage.

## Key features

- **Tabular encoding** — arrays of uniform objects emit keys once, not per row
- **Structural deduplication** — Merkle-style hashing factorizes recurring subtrees via `@s:` references
- **Minification** — semicolons encode depth changes, no whitespace needed
- **Human-readable** — readable without pretty-printing (unlike JSON)
- **No SIMD required** — runs on x86, ARM, ESP32, Arduino without platform-specific code

## Performance headline

**SCON tape decoder beats simd-json** — the fastest JSON parser — on structured data. Pure scalar code.

| Dataset | simd-json | serde_json | SCON tape | tape vs simd |
|---------|----------:|-----------:|----------:|-------------:|
| OpenAPI Specs (48 KB) | 0.246 ms | 0.418 ms | **0.195 ms** | **21% faster** |
| Config Records (72 KB) | 0.232 ms | 0.451 ms | 0.292 ms | 26% slower |
| DB Exports (19 KB) | 0.055 ms | 0.086 ms | **0.045 ms** | **18% faster** |
| Sparkplug B (7 KB) | 0.013 ms | 0.021 ms | **0.008 ms** | **38% faster** |
| IoT Telemetry (28 KB) | 0.045 ms | 0.084 ms | **0.044 ms** | **2% faster** |
| ISA-95 Equipment (52 KB) | 0.055 ms | 0.111 ms | **0.011 ms** | **80% faster** |

Rust, 500 iterations, release mode. SCON tape wins on 6 of 7 datasets. Config Records (deep nesting, low tabular content) is the one case where simd-json leads.

---

## Edge-to-Cloud: IoT and industrial telemetry

In bandwidth-constrained environments (satellite backhaul, LoRaWAN, 900 MHz mesh), transmission time dominates latency. SCON's tabular encoding reduces payload before transport — fewer bytes on the wire, fewer QUIC/TCP frames, less retransmission on lossy links.

Tested against real-world industrial fixtures:

| Dataset | Shape | SCON feature | Size vs JSON | Decode vs simd-json |
|---------|-------|--------------:|--------------------:|--------------------:|
| **ISA-95 Equipment** | Nested hierarchies (depth 13) | Tabular + minification | **-87%** (3.3 KB) | **5x faster** (0.011 ms) |
| **Sparkplug B** | MQTT telemetry batches | Tabular array | **-47%** (2.4 KB) | **1.6x faster** (0.008 ms) |
| **IoT Telemetry** | Flat sensor readings | Tabular array | **-29%** (12.0 KB) | **~parity** (0.044 ms) |

### Wire-to-parsed latency

Total time = `payload_bytes × 8 / bandwidth + decode_time`. This is what matters on satellite links and LoRa gateways.

**ISA-95 Equipment (SCON's strongest case — 87% smaller):**

| Bandwidth | JSON + simd-json | SCON(min) + tape | Saving |
|-----------|----------------:|-----------------:|-------:|
| 1 Mbps (LoRa/satellite) | 199.3 ms | **26.4 ms** | **-87%** |
| 10 Mbps (WiFi) | 20.0 ms | **2.6 ms** | **-87%** |
| 100 Mbps (Ethernet) | 2.0 ms | **0.3 ms** | **-86%** |

**Sparkplug B (MQTT metrics — 47% smaller):**

| Bandwidth | JSON + simd-json | SCON(min) + tape | Saving |
|-----------|----------------:|-----------------:|-------:|
| 1 Mbps (LoRa/satellite) | 35.2 ms | **19.2 ms** | **-45%** |
| 10 Mbps (WiFi) | 3.5 ms | **1.9 ms** | **-45%** |
| 100 Mbps (Ethernet) | 0.4 ms | **0.2 ms** | **-45%** |

### Resource budget per candidate

| Candidate | Payload (OpenAPI) | Decode time | Peak RAM | Needs SIMD | Embedded viable |
|-----------|------------------:|------------:|---------:|:----------:|:---------------:|
| JSON + simd-json | 49 KB | 0.246 ms | 4,443 KB | Yes (AVX2/NEON) | RPi only |
| JSON + serde_json | 49 KB | 0.418 ms | 4,676 KB | No | Yes |
| SCON(min) + tape | 42 KB | 0.195 ms | 3,874 KB | No | Yes |
| SCON(dedup+min) + tape | 17 KB | 0.195 ms | 3,874 KB | No | Yes |

SCON tape uses the least memory in all datasets — critical for ESP32 (520 KB SRAM), Arduino (2–32 KB), and battery-powered edge nodes.

---

## LLM context optimization (token efficiency)

For RAG pipelines, tool-use agents, and prompt engineering, JSON's structural characters and repetitive keys waste context window tokens and inflate API costs. SCON's deduplication and minified syntax reduce tokenization overhead.

Token counts using OpenAI's `cl100k_base` tokenizer (GPT-4 / GPT-3.5):

| Dataset | Format | Tokens | Reduction vs JSON |
|---------|--------|-------:|------------------:|
| OpenAPI Specs | JSON | ~12,500 | baseline |
| OpenAPI Specs | SCON(min) | ~7,500 | **-40%** |
| OpenAPI Specs | SCON(dedup+min) | **~4,500** | **-64%** |

How it works: `autoExtract` identifies recurring subtrees (e.g., identical response schemas, parameter blocks) and replaces them with `@s:name` references. The LLM sees the schema definition once, then compact references — maximizing effective context window.

**Payload comparison (all datasets):**

| Dataset | JSON | SCON(min) | Reduction | SCON(dedup+min) | Reduction |
|---------|-----:|----------:|----------:|----------------:|----------:|
| OpenAPI Specs | 49 KB | 42 KB | -13% | **17 KB** | **-66%** |
| Config Records | 73 KB | 66 KB | -8% | 65 KB | -11% |
| DB Exports | 20 KB | 14 KB | -29% | 14 KB | -29% |

Dedup shines on data with structural repetition (API specs, config schemas, equipment hierarchies). On already-unique data (DB exports), minification alone provides the win.

---

## Paper baseline vs current performance

The paper was published with pre-optimization numbers. The tape decoder (post-publication) dramatically improved decode performance:

| Metric | Paper baseline | Current (tape mode) |
|--------|---------------|---------------------|
| Payload size (no dedup) | 13–29% smaller | 13–29% smaller (unchanged) |
| Payload size (with dedup) | up to 66% smaller | up to **87% smaller** (ISA-95) |
| LLM tokens (cl100k_base) | 64% fewer | 64% fewer (unchanged) |
| Decode (Rust, vs serde_json) | 2.5–3.4x slower | **0.4–0.6x (faster)** |
| Decode (Rust, vs simd-json) | not measured | **0.8x on 6/7 datasets** |
| Encode (Rust) | 1.6–3.4x slower | 1.1–1.9x slower |

Break-even on bandwidth-limited links has moved from ~100 Mbps to effectively all bandwidths — SCON tape is now faster to decode *and* smaller to transmit.

## PHP native extension

The PHP extension uses the tape decoder internally, emitting Zvals directly from the tape — no intermediate AST (same architecture as PHP's built-in `json_decode`).

| Operation | Dataset | json_decode (C) | scon_decode (Rust ext) | Ratio |
|-----------|---------|----------------:|-----------------------:|------:|
| Decode | OpenAPI | 0.374 ms | 0.440 ms | 1.2x |
| Decode | DB | 0.093 ms | 0.097 ms | 1.1x |
| Encode | OpenAPI | 0.108 ms | 0.329 ms | 3.1x |

Decode is near parity with PHP's C `json_decode`. The Rust ext is **9–18x faster than PHP userland SCON**.

## Implementations

| Language | Type | Path |
|----------|------|------|
| **Rust** | Native crate (tape decoder, encoder, minifier) | `rs/` |
| **PHP ext** | Native extension (Rust via ext-php-rs) | `ext/` |
| **PHP** | Userland (encoder, decoder, dedup, schema registry) | `php/` |
| **JavaScript** | Userland (Node.js) | `js/` |

## Running benchmarks

```bash
php bench/generate_fixtures.php     # generate canonical fixtures
php bench/bench.php                 # PHP userland benchmark
node --expose-gc bench/bench.mjs    # JS benchmark
cargo build --release && ./target/release/scon-bench --iterations=500  # Rust benchmark

# PHP native extension
cargo build --release -p scon-php
php -d extension=./target/release/libscon_php.so bench/bench_ext.php
```

Benchmark suite includes 7 fixtures: OpenAPI Specs, Config Records, DB Exports, Sparkplug B, IoT Telemetry (2 sizes), ISA-95 Equipment. See [bench/README.md](bench/README.md) for detailed methodology and results.

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
