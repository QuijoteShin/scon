# SCON Benchmark Suite

Cross-language benchmark comparing SCON encode/decode performance against native JSON implementations.

## Datasets

Each benchmark generates the same three synthetic datasets for fair comparison:

| Dataset | Description | ~Size |
|---------|-------------|-------|
| **Config Records** | Structured config with nested objects | ~67 KB |
| **DB Exports** | Tabular DDL arrays (high field repetition) | ~70 KB |
| **API/OpenAPI Spec** | Endpoint definitions with schemas | ~40-50 KB |

The **10MB benchmark** (PHP only) adds three large-scale datasets: Server Logs, Geo API, and Multimedia Metadata.

## Running benchmarks

```bash
# PHP — standard datasets (~70 KB each)
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

Each run saves a JSON file to `bench/datasets/` with the naming convention:

```
bench/datasets/{lang}_{timestamp}.json
```

These files contain per-dataset metrics: payload sizes, encode/decode times (min, max, mean, median, p95, p99), ops/sec, and throughput in MB/s. They can be loaded for cross-language comparison.

## What is being compared

| Language | JSON impl | SCON impl |
|----------|-----------|-----------|
| **PHP** | `json_encode`/`json_decode` (C extension) | Userland PHP |
| **JavaScript** | `JSON.stringify`/`JSON.parse` (V8 native) | Userland JS |
| **Rust** | `serde_json` (native Rust) | `scon` crate (native Rust) |

The Rust benchmark provides the fairest format-vs-format comparison since both sides are native compiled code. PHP and JS benchmarks show real-world performance where SCON runs in userland against C/C++ JSON engines.

## Insights

> Results from standard benchmark runs (100 iterations, WSL2 Linux 5.15).

### Payload size: where SCON wins

| Dataset | JSON | SCON(min) | Reduction | SCON dedup | Dedup reduction |
|---------|-----:|----------:|----------:|-----------:|----------------:|
| DB Exports | 70.0 KB | 35.0 KB | **-50.0%** | 14.5 KB | **-79.3%** |
| OpenAPI Specs | 48.3 KB | 41.8 KB | -13.4% | 16.6 KB | **-65.9%** |
| Config Records | 67.2 KB | 62.2 KB | -7.4% | — | — |

SCON's tabular compression shines on repetitive structures (arrays of objects with identical keys). DB Exports halves in size without any compression algorithm — just structural deduplication. With dedup enabled, OpenAPI specs shrink by 66%.

After gzip, the gap narrows but SCON still wins:

| Dataset | JSON gzip | SCON(min) gzip |
|---------|----------:|---------------:|
| DB Exports | 1,460 B | 1,451 B |
| OpenAPI Specs | 1,289 B | 1,232 B |
| Config Records | 4,512 B | 4,438 B |

### Speed: Rust native (format-vs-format)

The fairest comparison — both serde_json and SCON are compiled Rust:

| Dataset | serde_json enc | SCON enc | Ratio | serde_json dec | SCON dec | Ratio |
|---------|---------------:|---------:|------:|---------------:|---------:|------:|
| Config Records | 0.061 ms | 0.122 ms | 2.0x | 0.247 ms | 0.765 ms | 3.1x |
| DB Exports | 0.079 ms | 0.287 ms | 3.6x | 0.416 ms | 0.888 ms | 2.1x |
| API Spec | 0.056 ms | 0.092 ms | 1.6x | 0.292 ms | 0.858 ms | 2.9x |

SCON encoding is 1.6–3.6x slower than serde_json; decoding 2.1–3.1x slower. This is the inherent cost of indent-based parsing vs delimiter-based. The Rust SCON decoder has room for optimization (SIMD, zero-copy).

### Speed: SCON Rust vs PHP/JS json (cross-language)

SCON in Rust native vs JSON in C extensions / V8 engine:

**Encoding (median ms)**

| Dataset | SCON Rust | PHP json_encode | JS JSON.stringify |
|---------|----------:|----------------:|------------------:|
| Config Records | 0.122 ms | 0.141 ms | 0.252 ms |
| DB Exports | 0.287 ms | 0.039 ms | 0.035 ms |

**Decoding (median ms)**

| Dataset | SCON Rust | PHP json_decode | JS JSON.parse |
|---------|----------:|----------------:|--------------:|
| Config Records | 0.765 ms | 0.481 ms | 0.303 ms |
| DB Exports | 0.888 ms | 0.108 ms | 0.081 ms |

Config Records: SCON Rust encode beats PHP json_encode (1.2x faster) and JS (2.1x faster). On decode PHP and JS native engines are faster.

DB Exports: PHP/JS json engines win on raw speed (7–8x faster), but SCON produces 50% smaller payloads — the network transmission savings can outweigh the parsing overhead depending on the use case.

### Throughput (Rust)

| Dataset | SCON encode | SCON decode | serde_json encode | serde_json decode |
|---------|------------:|------------:|------------------:|------------------:|
| Config Records | 532 MB/s | 85 MB/s | 1,080 MB/s | 265 MB/s |
| DB Exports | 121 MB/s | 39 MB/s | 861 MB/s | 164 MB/s |

### Key takeaways

1. **SCON's strength is payload size, not speed.** On tabular data (the most common pattern in APIs and databases), SCON achieves 50% reduction without compression. With dedup, up to 79%.

2. **The speed gap is the format's inherent cost.** Indent-based parsing requires more work than scanning for `{`, `}`, `,`. The 2–3x Rust-vs-Rust ratio represents the true format overhead, not an implementation deficiency.

3. **SCON Rust encode already competes with PHP/JS json_encode** on config-style data. A PHP FFI extension wrapping the Rust encoder would bring this performance to the PHP ecosystem.

4. **Network-bound workloads favor SCON.** When serialization time is dwarfed by transmission latency (APIs, microservices, edge), the 50% smaller payload is more valuable than microseconds of parsing.

5. **gzip equalizes size but not CPU.** After gzip both formats converge in size, but SCON starts from a smaller input — meaning less CPU spent on compression for the same result.
