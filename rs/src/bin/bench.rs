// scon/src/bin/bench.rs
// SCON Benchmark — fair comparison: Rust SCON vs Rust serde_json
// Usage: scon-bench [json_file]
// If no file, generates synthetic datasets

use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::io::Read;
use std::fs;
use std::path::Path;

// Direct imports from workspace
use scon_core::value::json_to_scon;
use scon_core::{Encoder, Decoder, Minifier, Value};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let iters_flag = args.iter().find(|a| a.starts_with("--iterations="));
    let iters: usize = iters_flag.map(|f| f.split('=').nth(1).unwrap().parse().unwrap()).unwrap_or(100);

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  SCON Benchmark — Rust Implementation                   ║");
    println!("║  Iterations: {} (warmup: 5){}║", iters, " ".repeat(33 - format!("{}", iters).len()));
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    let mut all_results: Vec<serde_json::Value> = Vec::new();

    if args.len() > 1 && !args[1].starts_with("--") {
        // Load JSON file
        let path = &args[1];
        let mut file = std::fs::File::open(path).expect("Cannot open file");
        let mut json_str = String::new();
        file.read_to_string(&mut json_str).expect("Cannot read file");
        let json_val: serde_json::Value = serde_json::from_str(&json_str).expect("Invalid JSON");
        let scon_val = json_to_scon(&json_val);
        let name = std::path::Path::new(path).file_stem().unwrap().to_str().unwrap();
        let result = run_benchmark(name, &json_str, &json_val, &scon_val, iters);
        all_results.push(result);
    } else {
        // Generate synthetic datasets matching PHP benchmarks
        let datasets = generate_datasets();
        for (name, json_str, json_val, scon_val) in &datasets {
            let result = run_benchmark(name, json_str, json_val, scon_val, iters);
            all_results.push(result);
            println!();
        }
        print_summary(&datasets);
    }

    // Save JSON results — incremental filename
    save_results(&all_results, iters);
}

fn run_benchmark(name: &str, json_str: &str, json_val: &serde_json::Value, scon_val: &Value, iters: usize) -> serde_json::Value {
    let warmup = 5;

    println!("━━━ {} ━━━", name);
    println!("  Input: {:.1} KB JSON", json_str.len() as f64 / 1024.0);

    // --- Encode ---
    let encoder = Encoder::new();

    // Warmup
    for _ in 0..warmup {
        let _ = serde_json::to_string(json_val).unwrap();
        let _ = encoder.encode(scon_val);
    }

    // JSON encode
    let mut json_encode_times = Vec::with_capacity(iters);
    let mut json_encoded = String::new();
    for _ in 0..iters {
        let start = Instant::now();
        json_encoded = serde_json::to_string(json_val).unwrap();
        json_encode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    // SCON encode
    let mut scon_encode_times = Vec::with_capacity(iters);
    let mut scon_encoded = String::new();
    for _ in 0..iters {
        let start = Instant::now();
        scon_encoded = encoder.encode(scon_val);
        scon_encode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    // SCON minify
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

    // --- Payload Size ---
    println!("  Payload Size:");
    println!("    JSON:             {:.1} KB", json_encoded.len() as f64 / 1024.0);
    let scon_pct = ((scon_encoded.len() as f64 / json_encoded.len() as f64) - 1.0) * 100.0;
    println!("    SCON:             {:.1} KB ({:+.1}%)", scon_encoded.len() as f64 / 1024.0, scon_pct);
    let min_pct = ((scon_minified.len() as f64 / json_encoded.len() as f64) - 1.0) * 100.0;
    println!("    SCON(min):        {:.1} KB ({:+.1}%)", scon_minified.len() as f64 / 1024.0, min_pct);

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
    // Warmup
    for _ in 0..warmup {
        let _: serde_json::Value = serde_json::from_str(&json_encoded).unwrap();
        let _ = Decoder::new().decode(&scon_encoded);
    }

    // JSON decode
    let mut json_decode_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _: serde_json::Value = serde_json::from_str(&json_encoded).unwrap();
        json_decode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    // SCON decode
    let mut scon_decode_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        let _ = Decoder::new().decode(&scon_encoded);
        scon_decode_times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }

    // SCON minified decode
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
    minify_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
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
            }
        }
        Err(e) => println!("  Roundtrip:          DECODE ERROR: {}", e),
    }

    // Minified roundtrip
    let min_decoded = Decoder::new().decode(&scon_minified);
    match min_decoded {
        Ok(_) => println!("  Min roundtrip:      OK"),
        Err(e) => println!("  Min roundtrip:      FAIL: {}", e),
    }

    // Return structured result
    serde_json::json!({
        "dataset": name,
        "payload": {
            "json": { "raw": json_encoded.len(), "gzip": null },
            "scon": { "raw": scon_encoded.len() },
            "scon_min": { "raw": scon_minified.len() },
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
        },
        "throughput": {
            "json_decode_mbs": (json_dec_tp * 100.0).round() / 100.0,
            "scon_decode_mbs": (scon_dec_tp * 100.0).round() / 100.0,
            "json_encode_mbs": (json_enc_tp * 100.0).round() / 100.0,
            "scon_encode_mbs": (scon_enc_tp * 100.0).round() / 100.0,
        },
    })
}

fn generate_datasets() -> Vec<(String, String, serde_json::Value, Value)> {
    let mut datasets = Vec::new();

    // 1. Config Records (~75KB) — flat key-value, typical app config
    {
        let mut config = serde_json::Map::new();
        let mut app_settings = serde_json::Map::new();
        for i in 0..200 {
            app_settings.insert(format!("setting_{}", i), serde_json::json!({
                "value": format!("value_{}", i),
                "type": if i % 3 == 0 { "string" } else if i % 3 == 1 { "integer" } else { "boolean" },
                "default": format!("default_{}", i),
                "description": format!("Configuration setting number {} for the application module", i),
                "category": format!("category_{}", i % 10),
                "required": i % 2 == 0
            }));
        }
        config.insert("app".to_string(), serde_json::Value::Object(app_settings));

        let mut db_settings = serde_json::Map::new();
        for i in 0..100 {
            db_settings.insert(format!("connection_{}", i), serde_json::json!({
                "host": format!("db-{}.internal.cluster", i),
                "port": 3306 + (i % 10),
                "database": format!("app_db_{}", i),
                "max_connections": 10 + i,
                "timeout": 30 + i
            }));
        }
        config.insert("databases".to_string(), serde_json::Value::Object(db_settings));

        let mut features = serde_json::Map::new();
        for i in 0..150 {
            features.insert(format!("feature_{}", i), serde_json::json!({
                "enabled": i % 3 != 0,
                "rollout_percentage": (i * 7) % 100,
                "description": format!("Feature flag {} controls the behavior of subsystem {}", i, i % 20)
            }));
        }
        config.insert("features".to_string(), serde_json::Value::Object(features));

        let json_val = serde_json::Value::Object(config);
        let json_str = serde_json::to_string(&json_val).unwrap();
        let scon_val = json_to_scon(&json_val);
        datasets.push(("Config Records".to_string(), json_str, json_val, scon_val));
    }

    // 2. DB Exports (~50KB) — tabular data (array of objects with uniform keys)
    {
        let mut tables = serde_json::Map::new();

        let roles = ["admin", "editor", "viewer", "moderator"];
        let mut users: Vec<serde_json::Value> = Vec::new();
        for i in 0..200 {
            users.push(serde_json::json!({
                "id": i + 1,
                "name": format!("User {}", i),
                "email": format!("user{}@example.com", i),
                "status": if i % 5 == 0 { "inactive" } else { "active" },
                "role": roles[i % 4],
                "created_at": format!("2024-01-{:02}T10:00:00Z", (i % 28) + 1),
                "login_count": i * 17 % 1000
            }));
        }
        tables.insert("users".to_string(), serde_json::Value::Array(users));

        let currencies = ["USD", "EUR", "GBP", "CLP"];
        let order_statuses = ["pending", "confirmed", "shipped", "delivered"];
        let mut orders: Vec<serde_json::Value> = Vec::new();
        for i in 0..300 {
            orders.push(serde_json::json!({
                "order_id": 1000 + i,
                "user_id": (i % 200) + 1,
                "total": format!("{:.2}", (i as f64 * 29.99) % 9999.99),
                "currency": currencies[i % 4],
                "status": order_statuses[i % 4],
                "items_count": (i % 10) + 1
            }));
        }
        tables.insert("orders".to_string(), serde_json::Value::Array(orders));

        let levels = ["INFO", "WARN", "ERROR", "DEBUG"];
        let modules = ["auth", "api", "db", "cache", "queue"];
        let mut logs: Vec<serde_json::Value> = Vec::new();
        for i in 0..100 {
            logs.push(serde_json::json!({
                "timestamp": format!("2024-03-01T{:02}:{:02}:{:02}Z", i % 24, (i * 7) % 60, (i * 13) % 60),
                "level": levels[i % 4],
                "message": format!("Event {} occurred in module {}", i, modules[i % 5]),
                "source": format!("worker-{}", i % 8)
            }));
        }
        tables.insert("logs".to_string(), serde_json::Value::Array(logs));

        let json_val = serde_json::Value::Object(tables);
        let json_str = serde_json::to_string(&json_val).unwrap();
        let scon_val = json_to_scon(&json_val);
        datasets.push(("DB Exports".to_string(), json_str, json_val, scon_val));
    }

    // 3. API Spec (~115KB) — deeply nested with repeated schemas
    {
        let mut spec = serde_json::Map::new();
        spec.insert("openapi".to_string(), serde_json::json!("3.1.0"));
        spec.insert("info".to_string(), serde_json::json!({
            "title": "Benchmark API",
            "version": "1.0.0",
            "description": "API specification for benchmark testing with multiple endpoints and schemas"
        }));

        let mut paths = serde_json::Map::new();
        let resources = ["users", "orders", "products", "categories", "reviews", "payments", "shipments", "notifications", "reports", "settings"];

        for resource in &resources {
            for action in &["list", "get", "create", "update", "delete", "search", "export"] {
                let path = format!("/api/{}/{}", resource, action);
                let method = match *action {
                    "list" | "get" | "search" | "export" => "get",
                    "create" => "post",
                    "update" => "put",
                    "delete" => "delete",
                    _ => "get",
                };

                let mut params: Vec<serde_json::Value> = Vec::new();
                params.push(serde_json::json!({
                    "name": "Authorization",
                    "in": "header",
                    "required": true,
                    "schema": {"type": "string"}
                }));
                if *action == "get" || *action == "update" || *action == "delete" {
                    params.push(serde_json::json!({
                        "name": "id",
                        "in": "path",
                        "required": true,
                        "schema": {"type": "integer"}
                    }));
                }
                if *action == "list" || *action == "search" {
                    params.push(serde_json::json!({"name": "page", "in": "query", "schema": {"type": "integer", "default": 1}}));
                    params.push(serde_json::json!({"name": "limit", "in": "query", "schema": {"type": "integer", "default": 20}}));
                }

                let mut endpoint = serde_json::Map::new();
                endpoint.insert("summary".to_string(), serde_json::json!(format!("{} {}", action, resource)));
                endpoint.insert("tags".to_string(), serde_json::json!([resource]));
                endpoint.insert("parameters".to_string(), serde_json::Value::Array(params));
                endpoint.insert("responses".to_string(), serde_json::json!({
                    "200": {"description": "Success", "content": {"application/json": {"schema": {"type": "object", "properties": {"success": {"type": "boolean"}, "data": {"type": "object"}}}}}},
                    "400": {"description": "Bad Request"},
                    "401": {"description": "Unauthorized"},
                    "404": {"description": "Not Found"},
                    "500": {"description": "Internal Server Error"}
                }));

                let mut path_item = serde_json::Map::new();
                path_item.insert(method.to_string(), serde_json::Value::Object(endpoint));
                paths.insert(path, serde_json::Value::Object(path_item));
            }
        }
        spec.insert("paths".to_string(), serde_json::Value::Object(paths));

        let json_val = serde_json::Value::Object(spec);
        let json_str = serde_json::to_string(&json_val).unwrap();
        let scon_val = json_to_scon(&json_val);
        datasets.push(("API Spec".to_string(), json_str, json_val, scon_val));
    }

    println!("Generated datasets:");
    for (name, json_str, _, _) in &datasets {
        println!("  {}: {:.1} KB JSON", name, json_str.len() as f64 / 1024.0);
    }
    println!();

    datasets
}

fn print_summary(_datasets: &[(String, String, serde_json::Value, Value)]) {
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║  CROSS-LANGUAGE COMPARISON                                                  ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("This Rust implementation provides a fair baseline for SCON performance.");
    println!("PHP json_encode/json_decode are C extensions — comparing them to PHP SCON");
    println!("(userland) is inherently unfair. The Rust comparison (native vs native)");
    println!("shows the true format overhead of SCON's indent-based parsing.");
    println!();
    println!("Key insight: If SCON encode/decode in Rust is only 2-5x slower than");
    println!("serde_json (also Rust), then a C extension for PHP SCON would achieve");
    println!("similar ratios vs json_encode — not the 15-30x seen in PHP userland.");
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

// Civil date from days since Unix epoch (no external crate needed)
fn epoch_days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
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

fn save_results(results: &[serde_json::Value], iters: usize) {
    // Find bench/datasets relative to the binary or use cwd
    let out_dir = Path::new("bench/datasets");
    if !out_dir.exists() {
        fs::create_dir_all(out_dir).ok();
    }

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    // YYYYMMDD_HHMMSS from epoch
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

    let output = serde_json::json!({
        "meta": {
            "lang": "rust",
            "suite": "standard",
            "iterations": iters,
            "date": &ts_str,
            "timestamp": ts,
            "hostname": hostname,
        },
        "results": results,
    });

    let filename = format!("rust_{}.json", ts_str);
    let out_path = out_dir.join(&filename);
    match fs::write(&out_path, serde_json::to_string_pretty(&output).unwrap()) {
        Ok(_) => println!("\nJSON results saved to: {}", out_path.display()),
        Err(e) => eprintln!("\nFailed to save results: {}", e),
    }
}
