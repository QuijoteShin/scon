// rs/src/bin/bench.rs
// SCON Benchmark — fair comparison: Rust SCON vs Rust serde_json
// Reads canonical fixtures from bench/fixtures/ (shared with PHP and JS benchmarks)
// Usage: scon-bench [--iterations=100]

use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::io::Write;
use std::fs;
use std::path::Path;

use scon_core::value::json_to_scon;
use scon_core::{Encoder, Decoder, Minifier, Value};
use flate2::write::GzEncoder;
use flate2::Compression;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let iters_flag = args.iter().find(|a| a.starts_with("--iterations="));
    let iters: usize = iters_flag.map(|f| f.split('=').nth(1).unwrap().parse().unwrap()).unwrap_or(100);

    let tag: Option<String> = args.iter()
        .find(|a| a.starts_with("--tag="))
        .map(|f| f.split('=').nth(1).unwrap().to_string());

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  SCON Benchmark — Rust Implementation                   ║");
    println!("║  Iterations: {} (warmup: 5){}║", iters, " ".repeat(33 - format!("{}", iters).len()));
    if let Some(ref t) = tag {
        let pad = 42usize.saturating_sub(t.len());
        println!("║  Tag: {}{}║", t, " ".repeat(pad));
    }
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    let mut all_results: Vec<serde_json::Value> = Vec::new();

    let datasets = load_fixtures();
    for (name, json_str, json_val, scon_val) in &datasets {
        let result = run_benchmark(name, json_str, json_val, scon_val, iters);
        all_results.push(result);
        println!();
    }

    print_summary();
    save_results(&all_results, iters, tag.as_deref());
}

fn load_fixtures() -> Vec<(String, String, serde_json::Value, Value)> {
    let fixture_dir = Path::new("bench/fixtures");
    let datasets_meta = [
        ("openapi_specs",  "OpenAPI Specs"),
        ("config_records", "Config Records"),
        ("db_exports",     "DB Exports"),
    ];

    println!("Loading fixtures from bench/fixtures/...");

    let mut datasets = Vec::new();
    for (slug, name) in &datasets_meta {
        let path = fixture_dir.join(format!("{}.json", slug));
        let json_str = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Fixture not found: {} — run: php bench/generate_fixtures.php", path.display()));
        let json_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let scon_val = json_to_scon(&json_val);
        let kb = json_str.len() as f64 / 1024.0;
        println!("  {}: {:.1} KB JSON", name, kb);
        datasets.push((name.to_string(), json_str, json_val, scon_val));
    }
    println!();
    datasets
}

fn gzip_size(data: &str) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data.as_bytes()).unwrap();
    encoder.finish().unwrap().len()
}

fn run_benchmark(name: &str, json_str: &str, json_val: &serde_json::Value, scon_val: &Value, iters: usize) -> serde_json::Value {
    let warmup = 5;

    println!("━━━ {} ━━━", name);
    println!("  Input: {:.1} KB JSON", json_str.len() as f64 / 1024.0);

    // --- Encode ---
    let encoder = Encoder::new();

    for _ in 0..warmup {
        let _ = serde_json::to_string(json_val).unwrap();
        let _ = encoder.encode(scon_val);
    }

    let mut json_encode_times = Vec::with_capacity(iters);
    let mut json_encoded = String::new();
    for _ in 0..iters {
        let start = Instant::now();
        json_encoded = serde_json::to_string(json_val).unwrap();
        json_encode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    let mut scon_encode_times = Vec::with_capacity(iters);
    let mut scon_encoded = String::new();
    for _ in 0..iters {
        let start = Instant::now();
        scon_encoded = encoder.encode(scon_val);
        scon_encode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    let mut minify_times = Vec::with_capacity(iters);
    let mut scon_minified = String::new();
    for _ in 0..iters {
        let start = Instant::now();
        scon_minified = Minifier::minify(&scon_encoded);
        minify_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    json_encode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    scon_encode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    minify_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    // --- Payload Size (with gzip and json_pretty) ---
    let json_pretty = serde_json::to_string_pretty(json_val).unwrap();

    let json_gz = gzip_size(&json_encoded);
    let json_pretty_gz = gzip_size(&json_pretty);
    let scon_gz = gzip_size(&scon_encoded);
    let scon_min_gz = gzip_size(&scon_minified);

    println!("  Payload Size:");
    println!("    JSON:             {:.1} KB (gzip: {:.1} KB)", json_encoded.len() as f64 / 1024.0, json_gz as f64 / 1024.0);
    println!("    JSON(pretty):     {:.1} KB (gzip: {:.1} KB)", json_pretty.len() as f64 / 1024.0, json_pretty_gz as f64 / 1024.0);
    let scon_pct = ((scon_encoded.len() as f64 / json_encoded.len() as f64) - 1.0) * 100.0;
    println!("    SCON:             {:.1} KB ({:+.1}%) (gzip: {:.1} KB)", scon_encoded.len() as f64 / 1024.0, scon_pct, scon_gz as f64 / 1024.0);
    let min_pct = ((scon_minified.len() as f64 / json_encoded.len() as f64) - 1.0) * 100.0;
    println!("    SCON(min):        {:.1} KB ({:+.1}%) (gzip: {:.1} KB)", scon_minified.len() as f64 / 1024.0, min_pct, scon_min_gz as f64 / 1024.0);

    // --- Encoding Time ---
    println!("  Encoding Time ({} iters):", iters);
    println!("    serde_json:       {:.3}ms (p95: {:.3}ms, p99: {:.3}ms) — {} ops/s",
        percentile(&json_encode_times, 50),
        percentile(&json_encode_times, 95),
        percentile(&json_encode_times, 99),
        ops_per_sec(&json_encode_times));
    println!("    SCON::encode:     {:.3}ms (p95: {:.3}ms, p99: {:.3}ms) — {} ops/s",
        percentile(&scon_encode_times, 50),
        percentile(&scon_encode_times, 95),
        percentile(&scon_encode_times, 99),
        ops_per_sec(&scon_encode_times));
    let encode_ratio = percentile(&scon_encode_times, 50) / percentile(&json_encode_times, 50);
    println!("    Ratio:            {:.1}x slower", encode_ratio);

    // --- Decode ---
    for _ in 0..warmup {
        let _: serde_json::Value = serde_json::from_str(&json_encoded).unwrap();
        let _ = Decoder::new().decode(&scon_encoded);
    }

    let mut json_decode_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _: serde_json::Value = serde_json::from_str(&json_encoded).unwrap();
        json_decode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    let mut scon_decode_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _ = Decoder::new().decode(&scon_encoded);
        scon_decode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    let mut scon_min_decode_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _ = Decoder::new().decode(&scon_minified);
        scon_min_decode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    json_decode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    scon_decode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    scon_min_decode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!("  Decoding Time ({} iters):", iters);
    println!("    serde_json:       {:.3}ms (p95: {:.3}ms, p99: {:.3}ms) — {} ops/s",
        percentile(&json_decode_times, 50),
        percentile(&json_decode_times, 95),
        percentile(&json_decode_times, 99),
        ops_per_sec(&json_decode_times));
    println!("    SCON::decode:     {:.3}ms (p95: {:.3}ms, p99: {:.3}ms) — {} ops/s",
        percentile(&scon_decode_times, 50),
        percentile(&scon_decode_times, 95),
        percentile(&scon_decode_times, 99),
        ops_per_sec(&scon_decode_times));
    println!("    SCON(min)decode:  {:.3}ms (p95: {:.3}ms, p99: {:.3}ms) — {} ops/s",
        percentile(&scon_min_decode_times, 50),
        percentile(&scon_min_decode_times, 95),
        percentile(&scon_min_decode_times, 99),
        ops_per_sec(&scon_min_decode_times));
    let decode_ratio = percentile(&scon_decode_times, 50) / percentile(&json_decode_times, 50);
    println!("    Ratio:            {:.1}x slower", decode_ratio);

    // --- Minify/Expand ---
    let mut expand_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _ = Minifier::expand(&scon_minified, 1);
        expand_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    expand_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!("  Minify/Expand ({} iters):", iters);
    println!("    minify:           {:.3}ms — {} ops/s",
        percentile(&minify_times, 50), ops_per_sec(&minify_times));
    println!("    expand:           {:.3}ms — {} ops/s",
        percentile(&expand_times, 50), ops_per_sec(&expand_times));

    // --- Throughput ---
    let json_dec_tp = json_encoded.len() as f64 / 1_048_576.0 / (percentile(&json_decode_times, 50) / 1000.0);
    let scon_dec_tp = scon_encoded.len() as f64 / 1_048_576.0 / (percentile(&scon_decode_times, 50) / 1000.0);
    let json_enc_tp = json_encoded.len() as f64 / 1_048_576.0 / (percentile(&json_encode_times, 50) / 1000.0);
    let scon_enc_tp = scon_encoded.len() as f64 / 1_048_576.0 / (percentile(&scon_encode_times, 50) / 1000.0);

    println!("  Throughput:");
    println!("    serde_json dec:   {:.1} MB/s", json_dec_tp);
    println!("    SCON::decode:     {:.1} MB/s", scon_dec_tp);
    println!("    serde_json enc:   {:.1} MB/s", json_enc_tp);
    println!("    SCON::encode:     {:.1} MB/s", scon_enc_tp);

    // --- Roundtrip verification ---
    let decoded = Decoder::new().decode(&scon_encoded);
    match decoded {
        Ok(ref val) => {
            let re_encoded = encoder.encode(val);
            let roundtrip_ok = re_encoded == scon_encoded;
            println!("  Roundtrip:          {}", if roundtrip_ok { "OK" } else { "FAIL" });
            if !roundtrip_ok {
                let first_diff = scon_encoded.chars().zip(re_encoded.chars())
                    .position(|(a, b)| a != b)
                    .unwrap_or(scon_encoded.len().min(re_encoded.len()));
                println!("    First diff at byte {}", first_diff);
                // Debug: dump context around diff
                let start = if first_diff > 80 { first_diff - 80 } else { 0 };
                let end = (first_diff + 80).min(scon_encoded.len()).min(re_encoded.len());
                println!("    ORIG: {:?}", &scon_encoded[start..end]);
                println!("    RENC: {:?}", &re_encoded[start..end]);
                println!("    Lengths: orig={} re={}", scon_encoded.len(), re_encoded.len());
            }
        }
        Err(e) => println!("  Roundtrip:          DECODE ERROR: {}", e),
    }

    let min_decoded = Decoder::new().decode(&scon_minified);
    match min_decoded {
        Ok(_) => println!("  Min roundtrip:      OK"),
        Err(e) => println!("  Min roundtrip:      FAIL: {}", e),
    }

    // Return structured result
    let size_savings_pct = if scon_encoded.len() > 0 {
        ((scon_encoded.len() as f64 - scon_minified.len() as f64) / scon_encoded.len() as f64 * 100.0 * 10.0).round() / 10.0
    } else { 0.0 };

    serde_json::json!({
        "dataset": name,
        "payload": {
            "json": { "raw": json_encoded.len(), "gzip": json_gz },
            "json_pretty": { "raw": json_pretty.len(), "gzip": json_pretty_gz },
            "scon": { "raw": scon_encoded.len(), "gzip": scon_gz },
            "scon_min": { "raw": scon_minified.len(), "gzip": scon_min_gz },
        },
        "encode": {
            "json": stats_json(&json_encode_times),
            "scon": stats_json(&scon_encode_times),
        },
        "decode": {
            "json": stats_json(&json_decode_times),
            "scon": stats_json(&scon_decode_times),
            "scon_min": stats_json(&scon_min_decode_times),
        },
        "minify_expand": {
            "minify": stats_json(&minify_times),
            "expand": stats_json(&expand_times),
            "size_savings_pct": size_savings_pct,
        },
        "throughput": {
            "json_decode_mbs": (json_dec_tp * 100.0).round() / 100.0,
            "scon_decode_mbs": (scon_dec_tp * 100.0).round() / 100.0,
            "json_encode_mbs": (json_enc_tp * 100.0).round() / 100.0,
            "scon_encode_mbs": (scon_enc_tp * 100.0).round() / 100.0,
        },
    })
}

fn print_summary() {
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║  CROSS-LANGUAGE COMPARISON                                                  ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("All benchmarks use identical fixtures from bench/fixtures/ (generated by PHP).");
    println!("Payload sizes (json.raw) must match exactly across PHP, JS, and Rust outputs.");
    println!();
    println!("This Rust implementation provides a fair baseline for SCON performance.");
    println!("PHP json_encode/json_decode are C extensions — comparing them to PHP SCON");
    println!("(userland) is inherently unfair. The Rust comparison (native vs native)");
    println!("isolates format-specific overhead from language runtime differences.");
}

// --- Stats helpers ---

fn percentile(sorted: &[f64], pct: usize) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = (sorted.len() * pct / 100).min(sorted.len() - 1);
    sorted[idx]
}

fn ops_per_sec(sorted: &[f64]) -> u64 {
    let median = percentile(sorted, 50);
    if median > 0.0 { (1000.0 / median) as u64 } else { 0 }
}

fn stats_json(sorted: &[f64]) -> serde_json::Value {
    let total: f64 = sorted.iter().sum();
    let n = sorted.len();
    serde_json::json!({
        "median": (percentile(sorted, 50) * 1000.0).round() / 1000.0,
        "p95": (percentile(sorted, 95) * 1000.0).round() / 1000.0,
        "p99": (percentile(sorted, 99) * 1000.0).round() / 1000.0,
        "mean": (total / n as f64 * 1000.0).round() / 1000.0,
        "min": (sorted.first().copied().unwrap_or(0.0) * 1000.0).round() / 1000.0,
        "max": (sorted.last().copied().unwrap_or(0.0) * 1000.0).round() / 1000.0,
        "ops_per_sec": ops_per_sec(sorted),
    })
}

fn save_results(results: &[serde_json::Value], iters: usize, tag: Option<&str>) {
    let out_dir = Path::new("bench/datasets");
    if !out_dir.exists() {
        fs::create_dir_all(out_dir).ok();
    }

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let secs = ts as i64;
    let days_since_epoch = secs / 86400;
    let time_of_day = (secs % 86400) as u32;
    let (year, month, day) = epoch_days_to_ymd(days_since_epoch);
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;
    let ts_str = format!("{:04}{:02}{:02}_{:02}{:02}{:02}", year, month, day, hour, min, sec);

    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    let mut meta = serde_json::json!({
        "lang": "rust",
        "suite": "standard",
        "fixture_source": "bench/fixtures/",
        "iterations": iters,
        "date": &ts_str,
        "timestamp": ts,
        "hostname": hostname,
    });
    if let Some(t) = tag {
        meta["tag"] = serde_json::Value::String(t.to_string());
    }

    let output = serde_json::json!({
        "meta": meta,
        "results": results,
    });

    let filename = match tag {
        Some(t) => format!("rust_{}_{}.json", t, ts_str),
        None => format!("rust_{}.json", ts_str),
    };
    let out_path = out_dir.join(&filename);
    match fs::write(&out_path, serde_json::to_string_pretty(&output).unwrap()) {
        Ok(_) => println!("\nJSON results saved to: {}", out_path.display()),
        Err(e) => eprintln!("\nFailed to save results: {}", e),
    }
}

fn epoch_days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
