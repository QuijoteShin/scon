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

## Benchmarks (vs JSON)

| Metric | Result |
|--------|--------|
| Payload size (no dedup) | 13–29% smaller |
| Payload size (with dedup) | up to 66% smaller |
| LLM tokens (cl100k_base) | 64% fewer |
| gzip CPU savings | 8–53% less |
| Encode overhead (Rust) | 1.6–3.4x slower |
| Decode overhead (Rust) | 2.5–3.4x slower |

Break-even: SCON is faster end-to-end on links below ~100 Mbps.

## Implementations

| Language | Type | Path |
|----------|------|------|
| **Rust** | Native crate | `rs/` |
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
