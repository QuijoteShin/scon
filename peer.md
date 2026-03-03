# peer.md
# SCON — Peer Review Preparation Guide

Consolidated analysis from Codex + Gemini oracles and internal review.
Reference for writing the academic paper (scon.tex).

---

## 1. Academic positioning

SCON occupies a **hybrid niche** between human-readable formats (JSON, YAML, TOML) and compact binary formats (Protobuf, MessagePack, CBOR). No existing format combines all of:

| Property | JSON | YAML | Protobuf | MsgPack | CBOR | SCON |
|----------|:----:|:----:|:--------:|:-------:|:----:|:----:|
| Human-readable | yes | yes | no | no | no | yes |
| No schema compiler required | yes | yes | no | yes | yes | yes |
| Tabular compaction | no | no | yes* | no | no | yes |
| Structural dedup (auto) | no | manual anchors | no | no | no | yes |
| In-band schema references | no | anchors | IDL (out-of-band) | no | no | yes |
| Schema override/patch | no | no | no | no | no | yes |
| Merkle-based structural diff | no | no | no | no | no | yes |
| Reversible minification | N/A | no | N/A | N/A | N/A | yes |

*Protobuf achieves tabular-like efficiency through field numbering, but requires pre-compiled IDL.

**Target use case**: Network-bound workloads with repetitive structures (REST APIs, DB exports, configuration) where I/O bandwidth is the bottleneck, not CPU serialization time.

**What SCON is NOT**: a speed competitor to binary formats or native JSON engines. The claim is strictly **payload efficiency + human readability**, not parse speed.

---

## 2. Original contributions

The novelty is in the **orchestration** of known concepts into a single coherent format:

### 2.1 Genuinely novel combinations

1. **Tabular inline hybrid**: Combining object semantics (key-value dictionaries) with relational/CSV semantics within the same document grammar, without breaking tree structure. An array of R uniform objects with K keys emits one header + R data rows, reducing key overhead from O(R×K×L_k) to O(K×L_k).

2. **Unsupervised auto-schema inference** (`autoExtract`): The encoder analyzes its own payload, identifies repeated sub-structures via xxHash128 fingerprinting, extracts them as named schemas, and replaces occurrences with references — all without human intervention or pre-compilation.

3. **Structural minification (dedent notation)**: Replacing JSON's closing brackets and YAML's whitespace with a unary depth-encoding system: `n` semicolons = dedent by `n-1` levels. This minimizes delimiter bytes while remaining fully reversible.

4. **Schema references with deep override**: `@s:name` references support dot-path overrides and field removals (`-field`), enabling schema inheritance patterns inline.

### 2.2 Built on existing ideas (acknowledge in paper)

- Merkle-tree hashing for equality/diff (well-established in git, IPFS, blockchain)
- Non-cryptographic fast hashing for dedup (xxHash family)
- Schema references and patching (analogous to JSON Schema `$ref`, YAML anchors, JSON Merge Patch)
- Indentation-based syntax (Python, YAML, Haskell)

---

## 3. Formal properties (publishable claims)

### 3.1 Tabular Compaction Ratio

For an array of R objects with K keys of average length L_k and average value length L_v:

```
JSON size   = R × (K × (L_k + L_v + 4) + 2)     // 4 = quotes + colon + comma; 2 = braces
SCON size   = (K × L_k + 2) + R × (K × L_v + K)  // header once + R data rows
Savings     = (R-1) × K × (L_k + 3)               // keys + delimiters eliminated R-1 times
```

When R >> 1 (common in APIs/DB exports), savings approach `K × (L_k + 3)` bytes per row.

**Empirical**: DB Exports fixture shows 29% reduction (SCON_min vs JSON_min) on 24 DDL tables.

### 3.2 Structural deduplication ratio

For data with S repeated sub-structures of average size M:

```
JSON size   = N_unique + S × M                    // each occurrence serialized fully
SCON_dedup  = N_unique + S × |ref| + |schema_def| // ref ≈ 10-20 bytes, schema_def ≈ M
Savings     = (S-1) × M - S × |ref|               // net savings when S ≥ 2 and M >> |ref|
```

**Empirical**: OpenAPI Specs shows 66% reduction with dedup+min vs JSON.

### 3.3 Readability-compactness trade-off

```
JSON_pretty / JSON_min   = 3.8x  (OpenAPI dataset)
SCON / JSON_min          = 1.17x (OpenAPI dataset)
```

SCON's standard format is human-readable with only 17% overhead over JSON minified, while JSON needs pretty-print (3.8x size increase) for the same readability.

### 3.4 Minification reversibility

`expand(minify(scon)) ≡ scon` — the minification is lossless and reversible. The unary semicolon encoding preserves exact tree structure.

### 3.5 Round-trip fidelity

`decode(encode(data)) ≡ data` — holds for the standard data model (primitives, arrays, objects).

**Caveat**: There are known edge cases where round-trip fails (see Section 5). The claim must be qualified: "Round-trip fidelity holds for the standard data model; edge cases involving arrays as first fields in list-item objects are documented as limitations."

---

## 4. Algorithmic complexity

### 4.1 Core operations

| Operation | Time | Space | Notes |
|-----------|------|-------|-------|
| Encode | O(N) | O(D + L) | Single-pass recursive DFS. Tabular detection O(R×K). |
| Decode | O(L) amortized | O(N + D) | Two-pass: line classification + monotonic-pointer parse. |
| Minify | O(L) | O(L) | Streaming, single-pass. |
| Expand | O(L) | O(L) | Character-by-character state machine. |
| json_to_scon | O(N) | O(N) | Structural transformation (Rust). |

### 4.2 TreeHash operations

| Operation | Time | Space | Hash |
|-----------|------|-------|------|
| hashTree (hybrid, current) | O(N × D) | O(N) | xxh128 |
| fingerprint (Merkle, optimal) | O(N × K × log K) | O(D) | xxh128 |
| diff (with Merkle pruning) | O(C × K) | O(D) | xxh128 |
| SchemaRegistry lookup | O(1) | O(S_total) | — |
| findMatchingSchema (encoder) | O(S × K) per node | O(1) | — |

### 4.3 Complexity caveats (from oracle review)

1. **Decoder worst case**: The monotonic pointer claim holds for well-formed input. On deeply nested structures, `nextIndex` scanning can degrade to O(L × D). For the paper, state this as "O(L) amortized; O(L × D) worst case on degenerate nesting."

2. **diff without memoization**: The current implementation recomputes fingerprints without caching. True O(C × K) requires memoized fingerprints. Mention as "O(C × K) with memoized fingerprints; current implementation recomputes, yielding O(N) in worst case."

3. **hashTree O(N × D) → O(N) optimization**: The hybrid approach (`json_encode` → xxh128) re-serializes ancestor nodes. Using bottom-up fingerprinting (already implemented as `fingerprint()`) collapses this to O(N). This optimization should be described in the paper as the theoretically optimal approach.

### 4.4 xxHash128 justification

| Property | xxHash128 | SHA-256 | CRC32 |
|----------|:---------:|:-------:|:-----:|
| Speed (GB/s) | >50 | ~0.5 | ~20 |
| Output bits | 128 | 256 | 32 |
| Collision bound (birthday) | 2⁻⁶⁴ | 2⁻¹²⁸ | 2⁻¹⁶ |
| Cryptographic | no | yes | no |
| Use case fit | structural dedup | integrity/security | checksums |

**Defense**: For non-adversarial structural comparison (encoding pipeline, not network security), 128-bit collision resistance at >50 GB/s is optimal. CRC32 is too collision-prone; SHA-256 is 100x slower without benefit for this use case.

**Paper recommendation**: Present xxHash128 as the "fast mode." Acknowledge that a "safe mode" (BLAKE3 or SHA-256) could be offered for adversarial contexts. Explicitly state the threat model: "SCON's dedup assumes non-adversarial input; collision-based attacks are out of scope."

---

## 5. Weaknesses and anticipated reviewer criticism

### 5.1 Performance gap

**Criticism**: "SCON is 2-3x slower than JSON even in native Rust. Why not just use JSON + gzip?"

**Response**: SCON's value proposition is payload size, not parse speed. In network-bound workloads (APIs, microservices, edge), serialization time is dwarfed by transmission latency. A 29-66% smaller payload reduces network I/O, CDN costs, and mobile data usage. The speed gap is the inherent cost of indent-based parsing vs delimiter scanning — a format trade-off, not an implementation deficiency.

**Additional experiment needed**: Measure `gzip(JSON)` vs `gzip(SCON)` compression CPU time. If SCON saves CPU during compression (smaller input → less compression work), this strengthens the argument.

### 5.2 gzip already solves key repetition

**Criticism**: "Sending JSON over gzip/Brotli already eliminates key redundancy."

**Response**:
- After gzip, JSON and SCON converge in size — but SCON starts from a smaller input, meaning less CPU spent on compression.
- SCON's tabular compaction is semantic (operates on the AST), while gzip is syntactic (byte-level LZ77 window). They are complementary, not competing.
- **Experiment needed**: Shannon entropy measurement. SCON_min should show higher information density (bits/byte) than JSON_min, indicating the data is closer to its theoretical entropy limit before compression.

### 5.3 `json_encode` dependency in hashTree

**Criticism**: "A new serialization format that internally depends on JSON for its own hash computation is circular."

**Response**: The `json_encode` path in `collectHashesHybrid` is a pragmatic optimization exploiting C-level serialization in PHP/V8. The theoretically clean approach is the `fingerprint()` method (bottom-up Merkle), which is already implemented and does not depend on JSON.

**Paper recommendation**: Present only `fingerprint()` as the canonical algorithm. Mention the `json_encode` hybrid as an "implementation optimization in interpreted languages" in a footnote or appendix.

### 5.4 Round-trip edge cases

**Known failure**: Objects in list items whose first field is a primitive array (`- deps[1]: a`) cause ambiguous parsing. The encoder emits syntax that the decoder interprets as an array header rather than a key-value pair.

**Mitigation**: Document as a known limitation. Propose disambiguation syntax (e.g., quoting the key: `- "deps[1]": a`) as future work.

### 5.5 Rust implementation gaps

- No `autoExtract`/dedup (TreeHash not ported)
- No schema/directive parsing in decoder (skips `s:`, `r:`, `sec:`, `@use` lines)
- `xxhash-rust` dependency declared but unused

**For the paper**: Present Rust as the "reference native implementation for performance measurement." Dedup/schema features are demonstrated via PHP/JS. This is honest and defensible.

### 5.6 Memory overhead of autoExtract

**Criticism**: "autoExtract requires loading the full tree in memory, preventing streaming encoding."

**Response**: autoExtract is opt-in. Standard `encode()` is single-pass streaming. The memory cost is the trade-off for automatic structural analysis. For streaming use cases, `encode()` without autoExtract is the correct choice.

---

## 6. Suggested experiments for publication

### 6.1 Must-have (strengthen core claims)

| Experiment | Purpose | Effort |
|------------|---------|--------|
| Shannon entropy (bits/byte) | Prove SCON_min is closer to data entropy limit | Low |
| gzip/Brotli CPU time comparison | Prove SCON saves compression CPU | Medium |
| Peak memory during autoExtract (10MB) | Quantify dedup memory cost | Low |
| Statistical significance (CI, multiple machines) | Academic rigor | Medium |

### 6.2 Should-have (strengthen novelty)

| Experiment | Purpose | Effort |
|------------|---------|--------|
| Real-world datasets (GeoJSON, production logs, public APIs) | Validate beyond synthetic data | Medium |
| Property-based / fuzz testing (encode→decode→encode) | Prove round-trip correctness | Medium |
| Pathological inputs (extreme depth, conflicting keys) | Prove parser robustness | Low |
| Component-level timing (parse vs schema vs autoExtract vs minify) | Identify bottlenecks | Low |

### 6.3 Nice-to-have (differentiation)

| Experiment | Purpose | Effort |
|------------|---------|--------|
| Collision experiment (millions of subtrees) | Validate xxHash128 collision bound | Low |
| Comparison with YAML anchors on dedup efficiency | Position vs closest competitor | Medium |
| Streaming encode benchmark (no autoExtract) | Show streaming viability | Low |
| Mobile/edge bandwidth savings simulation | Real-world impact | High |

---

## 7. Paper structure recommendation

```
1. Introduction
   - Problem: JSON's structural redundancy in repetitive data
   - Contribution: SCON — human-readable format with semantic compaction

2. Background and Related Work
   - Serialization formats taxonomy (text vs binary vs schema-based)
   - Structural compression in databases (columnar stores, dictionary encoding)
   - Merkle trees for structural comparison

3. SCON Format Specification
   - Syntax and grammar
   - Tabular encoding rule
   - Schema references and overrides
   - Minification (unary dedent notation)

4. Algorithms
   - Encoder (DFS, tabular detection) — O(N)
   - Decoder (two-pass, monotonic pointer) — O(L)
   - TreeHash: fingerprint (Merkle bottom-up) — O(N)
   - AutoExtract: schema inference — O(N) with fingerprint optimization

5. Implementation
   - Three implementations: PHP (userland), JS (userland), Rust (native)
   - Cross-language fixture methodology

6. Evaluation
   - Datasets and methodology
   - Payload size comparison (4 variants + dedup)
   - Encoding/decoding performance (3 languages)
   - Shannon entropy analysis
   - Compression interaction (gzip/Brotli CPU)
   - Memory profiling

7. Discussion
   - When to use SCON vs JSON vs binary formats
   - Limitations and edge cases
   - Threat model for hash-based dedup

8. Conclusion and Future Work
   - Rust TreeHash port
   - Streaming autoExtract
   - Formal grammar specification (BNF)
```

---

## 8. Key numbers for the paper

From benchmark fixtures (canonical, srand(42)):

| Metric | OpenAPI Specs | Config Records | DB Exports |
|--------|-------------:|---------------:|-----------:|
| JSON (min) | 49,067 B | 73,183 B | 19,688 B |
| JSON (pretty) | 182,616 B | — | — |
| SCON | 56,700 B | 74,529 B | 14,845 B |
| SCON (min) | 41,800 B | 67,049 B | 13,917 B |
| SCON (dedup+min) | 16,600 B | — | — |
| SCON_min vs JSON_min | -13.5% | -8.4% | -29.3% |
| SCON_dedup_min vs JSON_min | -65.7% | — | — |
| JSON_pretty / JSON_min | 3.8x | — | — |
| SCON / JSON_min | 1.17x | 1.02x | 0.75x |

Rust-vs-Rust (format overhead, 100 iterations):

| Metric | OpenAPI | Config | DB Exports |
|--------|-------:|-------:|-----------:|
| Encode ratio (SCON/serde_json) | 1.6x | 2.0x | 3.6x |
| Decode ratio (SCON/serde_json) | 2.9x | 3.1x | 2.1x |

---

## Sources

- Codex analysis: `.oraculo/codex_scon_paper_analysis_20260302_2229.txt`
- Gemini analysis: `.oraculo/gemini_scon_paper_analysis_20260302_2229.txt`
- Benchmark data: `bench/datasets/*.json`
- Fixtures: `bench/fixtures/*.json`
