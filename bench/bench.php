#!/usr/bin/env php
<?php
# bench/bench.php
# SCON Comprehensive Benchmark Suite
# Usage: php bench/bench.php [--iterations=100] [--output=json|table|both]
#
# Datasets:
#   1. OpenAPI Specs  — real endpoint or synthetic (if endpoint unavailable)
#   2. Config Records — synthetic structured config
#   3. DB Exports     — synthetic DDL array

$baseDir = dirname(__DIR__) . '/php';
require_once "$baseDir/bootstrap.php";
require_once "$baseDir/Scon.php";
require_once "$baseDir/Encoder.php";
require_once "$baseDir/Decoder.php";
require_once "$baseDir/Minifier.php";
require_once "$baseDir/SchemaRegistry.php";
require_once "$baseDir/Validator.php";
require_once "$baseDir/TreeHash.php";

use bX\Scon\Scon;

# ============================================================================
# CLI args
# ============================================================================
$args = getopt('', ['iterations:', 'output:', 'help']);
if (isset($args['help'])) {
    echo "Usage: php bench_scon.php [--iterations=100] [--output=json|table|both]\n";
    exit(0);
}
$ITERATIONS = (int)($args['iterations'] ?? 100);
$OUTPUT_MODE = $args['output'] ?? 'both';

echo "╔══════════════════════════════════════════════════════════╗\n";
echo "║  SCON Benchmark Suite — PHP " . PHP_VERSION . str_repeat(' ', 24 - strlen(PHP_VERSION)) . "║\n";
echo "║  Iterations: {$ITERATIONS}" . str_repeat(' ', 42 - strlen((string)$ITERATIONS)) . "║\n";
echo "╚══════════════════════════════════════════════════════════╝\n\n";

# ============================================================================
# 1. DATASET GENERATION
# ============================================================================

function generateDatasets(): array {
    $datasets = [];
    srand(42); # Deterministic for reproducible benchmarks

    # --- Dataset 1: OpenAPI Specs (synthetic ~115KB) ---
    $spec = [
        'openapi' => '3.1.0',
        'info' => ['title' => 'Benchmark API', 'version' => '1.0.0', 'description' => 'API specification for benchmark testing'],
        'paths' => [],
    ];
    $resources = ['users', 'orders', 'products', 'categories', 'reviews', 'payments', 'shipments', 'notifications', 'reports', 'settings'];
    $actions = ['list', 'get', 'create', 'update', 'delete', 'search', 'export'];
    $methods = ['list' => 'get', 'get' => 'get', 'create' => 'post', 'update' => 'put', 'delete' => 'delete', 'search' => 'get', 'export' => 'get'];
    foreach ($resources as $resource) {
        foreach ($actions as $action) {
            $path = "/api/{$resource}/{$action}";
            $params = [['name' => 'Authorization', 'in' => 'header', 'required' => true, 'schema' => ['type' => 'string']]];
            if (in_array($action, ['get', 'update', 'delete'])) {
                $params[] = ['name' => 'id', 'in' => 'path', 'required' => true, 'schema' => ['type' => 'integer']];
            }
            if (in_array($action, ['list', 'search'])) {
                $params[] = ['name' => 'page', 'in' => 'query', 'schema' => ['type' => 'integer', 'default' => 1]];
                $params[] = ['name' => 'limit', 'in' => 'query', 'schema' => ['type' => 'integer', 'default' => 20]];
                $params[] = ['name' => 'sort', 'in' => 'query', 'schema' => ['type' => 'string', 'default' => 'created_at']];
            }
            $spec['paths'][$path] = [$methods[$action] => [
                'summary' => ucfirst($action) . ' ' . $resource,
                'tags' => [$resource],
                'parameters' => $params,
                'responses' => [
                    '200' => ['description' => 'Success', 'content' => ['application/json' => ['schema' => ['type' => 'object', 'properties' => ['success' => ['type' => 'boolean'], 'data' => ['type' => 'object']]]]]],
                    '400' => ['description' => 'Bad Request', 'content' => ['application/json' => ['schema' => ['type' => 'object', 'properties' => ['error' => ['type' => 'string']]]]]],
                    '401' => ['description' => 'Unauthorized'],
                    '404' => ['description' => 'Not Found'],
                    '500' => ['description' => 'Internal Server Error'],
                ],
            ]];
        }
    }
    $datasets['OpenAPI Specs'] = $spec;

    # --- Dataset 2: Config Records (synthetic ~75KB) ---
    $services = ['auth', 'billing', 'notifications', 'analytics', 'search', 'storage', 'cache', 'queue', 'email', 'sms'];
    $envs = ['production', 'staging', 'development', 'testing'];
    $configRecords = ['services' => [], 'feature_flags' => []];
    foreach ($services as $svc) {
        foreach ($envs as $env) {
            $configRecords['services'][] = [
                'service_name' => $svc,
                'environment' => $env,
                'host' => "svc-{$svc}.{$env}.internal",
                'port' => rand(3000, 9000),
                'replicas' => rand(1, 8),
                'health_check' => "/health",
                'timeout_ms' => rand(1000, 30000),
                'retry_policy' => ['max_retries' => rand(1, 5), 'backoff_ms' => rand(100, 2000)],
                'tls' => $env === 'production',
                'log_level' => $env === 'production' ? 'ERROR' : 'DEBUG',
                'rate_limit' => ['requests_per_minute' => rand(100, 10000), 'burst' => rand(10, 100)],
                'dependencies' => array_slice($services, 0, rand(1, 4)),
                'metadata' => [
                    'version' => rand(1, 5) . '.' . rand(0, 20) . '.' . rand(0, 100),
                    'deployed_at' => '2026-02-' . str_pad(rand(1, 28), 2, '0', STR_PAD_LEFT) . 'T10:00:00',
                    'deployed_by' => 'ci/cd-pipeline-' . rand(100, 999),
                ],
            ];
        }
    }
    $flagNames = ['dark_mode', 'beta_ui', 'new_checkout', 'ai_assistant', 'realtime_sync', 'export_pdf', 'bulk_import', 'webhooks', 'api_v2', 'sso_oauth'];
    for ($i = 0; $i < 200; $i++) {
        $configRecords['feature_flags'][] = [
            'flag_name' => 'feature.' . $flagNames[rand(0, 9)] . '.' . rand(1, 50),
            'enabled' => (bool)rand(0, 1),
            'rollout_percentage' => rand(0, 100),
            'targeting_rules' => [
                ['attribute' => 'plan', 'operator' => 'in', 'values' => ['pro', 'enterprise']],
                ['attribute' => 'country', 'operator' => 'eq', 'value' => ['CL', 'US', 'MX', 'AR'][rand(0, 3)]],
            ],
            'created_at' => '2025-' . str_pad(rand(1, 12), 2, '0', STR_PAD_LEFT) . '-' . str_pad(rand(1, 28), 2, '0', STR_PAD_LEFT),
            'updated_at' => '2026-03-01T12:00:00',
        ];
    }
    $datasets['Config Records'] = $configRecords;

    # --- Dataset 3: DB Exports (synthetic ~50KB) ---
    $types = ['INT UNSIGNED', 'BIGINT UNSIGNED', 'VARCHAR(255)', 'VARCHAR(2000)', 'TEXT', 'DATETIME', 'TINYINT(1)', 'DECIMAL(12,2)'];
    $tables = [];
    $tableNames = ['users', 'profiles', 'entities', 'entity_relationships', 'orders', 'order_items', 'products', 'categories',
        'payments', 'payment_methods', 'invoices', 'workorders', 'work_units', 'notes', 'files', 'audit_log',
        'roles', 'role_packages', 'permissions', 'sessions', 'settings', 'notifications', 'email_queue', 'data_values_history'];
    foreach ($tableNames as $tn) {
        $colCount = rand(5, 15);
        $columns = [['name' => 'id', 'definition' => 'INT UNSIGNED AUTO_INCREMENT PRIMARY KEY']];
        $colNames = ['scope_entity_id', 'name', 'status', 'type', 'code', 'description', 'amount', 'total_clp',
            'created_at', 'updated_at', 'created_by', 'email', 'phone', 'parent_id', 'sort_order'];
        for ($c = 0; $c < min($colCount, count($colNames)); $c++) {
            $columns[] = ['name' => $colNames[$c], 'definition' => $types[rand(0, count($types) - 1)] . (rand(0, 1) ? ' NOT NULL' : ' DEFAULT NULL')];
        }
        $indexes = ['PRIMARY KEY (`id`)', "KEY `idx_scope` (`scope_entity_id`)"];
        if (rand(0, 1)) $indexes[] = "KEY `idx_status` (`status`)";
        if (rand(0, 1)) $indexes[] = "UNIQUE KEY `idx_code` (`code`)";
        $tables[] = ['table_name' => $tn, 'column_count' => count($columns), 'columns' => $columns, 'indexes' => $indexes];
    }
    $datasets['DB Exports'] = ['schema_version' => '2026-03-02', 'database' => 'benchmark_db', 'tables' => $tables];

    return $datasets;
}

echo "Generating datasets...\n";
$datasets = generateDatasets();
foreach ($datasets as $name => $data) {
    $jsonSize = strlen(json_encode($data));
    echo "  {$name}: " . number_format($jsonSize) . " bytes JSON (" . count((array)$data) . " top-level keys)\n";
}
echo "\n";

# ============================================================================
# 2. BENCHMARK FUNCTIONS
# ============================================================================

function formatBytes(int $bytes): string {
    if ($bytes >= 1048576) return number_format($bytes / 1048576, 2) . ' MB';
    if ($bytes >= 1024) return number_format($bytes / 1024, 1) . ' KB';
    return number_format($bytes) . ' B';
}

function percentChange(int $baseline, int $value): string {
    if ($baseline === 0) return 'N/A';
    $pct = (($value - $baseline) / $baseline) * 100;
    $sign = $pct < 0 ? '' : '+';
    return $sign . number_format($pct, 1) . '%';
}

function benchmarkTiming(callable $fn, int $iterations): array {
    # Warmup
    for ($i = 0; $i < min(5, $iterations); $i++) $fn();

    $times = [];
    for ($i = 0; $i < $iterations; $i++) {
        $start = hrtime(true);
        $fn();
        $times[] = (hrtime(true) - $start) / 1e6; # ms
    }
    sort($times);
    $count = count($times);
    return [
        'min' => $times[0],
        'max' => $times[$count - 1],
        'mean' => array_sum($times) / $count,
        'median' => $times[(int)($count * 0.5)],
        'p95' => $times[(int)($count * 0.95)],
        'p99' => $times[(int)($count * 0.99)],
        'total' => array_sum($times),
        'ops_per_sec' => $count / (array_sum($times) / 1000),
    ];
}

function benchmarkMemory(callable $fn): int {
    # Run once to warm caches, then discard
    $fn();
    gc_collect_cycles();
    gc_disable();
    $before = memory_get_usage();
    $result = $fn();
    $after = memory_get_usage();
    gc_enable();
    gc_collect_cycles();
    unset($result);
    return max(0, $after - $before);
}

# ============================================================================
# 3. RUN BENCHMARKS
# ============================================================================

$results = [];

foreach ($datasets as $datasetName => $data) {
    echo "━━━ {$datasetName} ━━━\n";
    $r = ['dataset' => $datasetName];

    # --- Payload Size ---
    $jsonStr = json_encode($data);
    $jsonSize = strlen($jsonStr);
    $jsonGzipSize = strlen(gzencode($jsonStr, 9));

    $sconStr = Scon::encode($data);
    $sconSize = strlen($sconStr);
    $sconGzipSize = strlen(gzencode($sconStr, 9));

    $sconMinStr = Scon::minify($sconStr);
    $sconMinSize = strlen($sconMinStr);
    $sconMinGzipSize = strlen(gzencode($sconMinStr, 9));

    # SCON with autoExtract (dedup)
    $sconDedupStr = Scon::encode($data, ['autoExtract' => true]);
    $sconDedupSize = strlen($sconDedupStr);
    $sconDedupGzipSize = strlen(gzencode($sconDedupStr, 9));

    $sconDedupMinStr = Scon::minify($sconDedupStr);
    $sconDedupMinSize = strlen($sconDedupMinStr);

    $r['payload'] = [
        'json' => ['raw' => $jsonSize, 'gzip' => $jsonGzipSize],
        'scon' => ['raw' => $sconSize, 'gzip' => $sconGzipSize],
        'scon_min' => ['raw' => $sconMinSize, 'gzip' => $sconMinGzipSize],
        'scon_dedup' => ['raw' => $sconDedupSize, 'gzip' => $sconDedupGzipSize],
        'scon_dedup_min' => ['raw' => $sconDedupMinSize],
    ];

    echo "  Payload Size:\n";
    echo "    JSON:             " . formatBytes($jsonSize) . " (gzip: " . formatBytes($jsonGzipSize) . ")\n";
    echo "    SCON:             " . formatBytes($sconSize) . " (" . percentChange($jsonSize, $sconSize) . ") (gzip: " . formatBytes($sconGzipSize) . ")\n";
    echo "    SCON(min):        " . formatBytes($sconMinSize) . " (" . percentChange($jsonSize, $sconMinSize) . ") (gzip: " . formatBytes($sconMinGzipSize) . ")\n";
    echo "    SCON(dedup):      " . formatBytes($sconDedupSize) . " (" . percentChange($jsonSize, $sconDedupSize) . ")\n";
    echo "    SCON(dedup+min):  " . formatBytes($sconDedupMinSize) . " (" . percentChange($jsonSize, $sconDedupMinSize) . ")\n";

    # --- Deduplication Ratio ---
    $dedupReduction = $sconSize > 0 ? (($sconSize - $sconDedupSize) / $sconSize) * 100 : 0;
    $r['dedup_ratio'] = [
        'scon_plain' => $sconSize,
        'scon_dedup' => $sconDedupSize,
        'reduction_pct' => round($dedupReduction, 1),
    ];
    echo "    Dedup reduction:  " . number_format($dedupReduction, 1) . "% additional savings\n";

    # --- Encoding Time ---
    echo "  Encoding Time ({$ITERATIONS} iterations):\n";

    $jsonEnc = benchmarkTiming(fn() => json_encode($data), $ITERATIONS);
    echo "    json_encode:      " . number_format($jsonEnc['median'], 3) . "ms (p95: " . number_format($jsonEnc['p95'], 3) . "ms, p99: " . number_format($jsonEnc['p99'], 3) . "ms) — " . number_format($jsonEnc['ops_per_sec'], 0) . " ops/s\n";

    $sconEnc = benchmarkTiming(fn() => Scon::encode($data), $ITERATIONS);
    echo "    SCON::encode:     " . number_format($sconEnc['median'], 3) . "ms (p95: " . number_format($sconEnc['p95'], 3) . "ms, p99: " . number_format($sconEnc['p99'], 3) . "ms) — " . number_format($sconEnc['ops_per_sec'], 0) . " ops/s\n";

    $sconDedupEnc = benchmarkTiming(fn() => Scon::encode($data, ['autoExtract' => true]), $ITERATIONS);
    echo "    SCON(dedup):      " . number_format($sconDedupEnc['median'], 3) . "ms (p95: " . number_format($sconDedupEnc['p95'], 3) . "ms, p99: " . number_format($sconDedupEnc['p99'], 3) . "ms) — " . number_format($sconDedupEnc['ops_per_sec'], 0) . " ops/s\n";

    $r['encode'] = ['json' => $jsonEnc, 'scon' => $sconEnc, 'scon_dedup' => $sconDedupEnc];

    # --- Decoding Time ---
    echo "  Decoding Time ({$ITERATIONS} iterations):\n";

    $jsonDec = benchmarkTiming(fn() => json_decode($jsonStr, true), $ITERATIONS);
    echo "    json_decode:      " . number_format($jsonDec['median'], 3) . "ms (p95: " . number_format($jsonDec['p95'], 3) . "ms, p99: " . number_format($jsonDec['p99'], 3) . "ms) — " . number_format($jsonDec['ops_per_sec'], 0) . " ops/s\n";

    $sconDec = benchmarkTiming(fn() => Scon::decode($sconStr), $ITERATIONS);
    echo "    SCON::decode:     " . number_format($sconDec['median'], 3) . "ms (p95: " . number_format($sconDec['p95'], 3) . "ms, p99: " . number_format($sconDec['p99'], 3) . "ms) — " . number_format($sconDec['ops_per_sec'], 0) . " ops/s\n";

    $sconMinDec = benchmarkTiming(fn() => Scon::decode($sconMinStr), $ITERATIONS);
    echo "    SCON(min)decode:  " . number_format($sconMinDec['median'], 3) . "ms (p95: " . number_format($sconMinDec['p95'], 3) . "ms, p99: " . number_format($sconMinDec['p99'], 3) . "ms) — " . number_format($sconMinDec['ops_per_sec'], 0) . " ops/s\n";

    $r['decode'] = ['json' => $jsonDec, 'scon' => $sconDec, 'scon_min' => $sconMinDec];

    # --- Minify/Expand Overhead ---
    echo "  Minify/Expand ({$ITERATIONS} iterations):\n";

    $minBench = benchmarkTiming(fn() => Scon::minify($sconStr), $ITERATIONS);
    echo "    minify:           " . number_format($minBench['median'], 3) . "ms — " . number_format($minBench['ops_per_sec'], 0) . " ops/s\n";

    $expBench = benchmarkTiming(fn() => Scon::expand($sconMinStr), $ITERATIONS);
    echo "    expand:           " . number_format($expBench['median'], 3) . "ms — " . number_format($expBench['ops_per_sec'], 0) . " ops/s\n";

    $r['minify_expand'] = [
        'minify' => $minBench,
        'expand' => $expBench,
        'size_savings_pct' => round((($sconSize - $sconMinSize) / $sconSize) * 100, 1),
    ];

    # --- Memory (Peak RSS) ---
    echo "  Memory (Peak RSS delta):\n";

    $jsonMem = benchmarkMemory(fn() => json_decode($jsonStr, true));
    echo "    json_decode:      " . formatBytes($jsonMem) . "\n";

    $sconMem = benchmarkMemory(fn() => Scon::decode($sconStr));
    echo "    SCON::decode:     " . formatBytes($sconMem) . "\n";

    $jsonEncMem = benchmarkMemory(fn() => json_encode($data));
    echo "    json_encode:      " . formatBytes($jsonEncMem) . "\n";

    $sconEncMem = benchmarkMemory(fn() => Scon::encode($data));
    echo "    SCON::encode:     " . formatBytes($sconEncMem) . "\n";

    $r['memory'] = [
        'json_decode' => $jsonMem,
        'scon_decode' => $sconMem,
        'json_encode' => $jsonEncMem,
        'scon_encode' => $sconEncMem,
    ];

    # --- Throughput (MB/s) ---
    $jsonDecMBs = ($jsonSize / 1048576) * $jsonDec['ops_per_sec'];
    $sconDecMBs = ($sconSize / 1048576) * $sconDec['ops_per_sec'];
    $jsonEncMBs = ($jsonSize / 1048576) * $jsonEnc['ops_per_sec'];
    $sconEncMBs = ($sconSize / 1048576) * $sconEnc['ops_per_sec'];

    echo "  Throughput:\n";
    echo "    json_decode:      " . number_format($jsonDecMBs, 2) . " MB/s\n";
    echo "    SCON::decode:     " . number_format($sconDecMBs, 2) . " MB/s\n";
    echo "    json_encode:      " . number_format($jsonEncMBs, 2) . " MB/s\n";
    echo "    SCON::encode:     " . number_format($sconEncMBs, 2) . " MB/s\n";

    $r['throughput'] = [
        'json_decode_mbs' => round($jsonDecMBs, 2),
        'scon_decode_mbs' => round($sconDecMBs, 2),
        'json_encode_mbs' => round($jsonEncMBs, 2),
        'scon_encode_mbs' => round($sconEncMBs, 2),
    ];

    $results[] = $r;
    echo "\n";
}

# ============================================================================
# 4. MSGPACK BASELINE (if ext-msgpack available)
# ============================================================================

echo "━━━ MessagePack Baseline ━━━\n";
if (function_exists('msgpack_pack')) {
    foreach ($datasets as $datasetName => $data) {
        $packed = msgpack_pack($data);
        $packedSize = strlen($packed);
        $jsonSize = strlen(json_encode($data));
        echo "  {$datasetName}: " . formatBytes($packedSize) . " (" . percentChange($jsonSize, $packedSize) . " vs JSON)\n";

        $mpEnc = benchmarkTiming(fn() => msgpack_pack($data), $ITERATIONS);
        $mpDec = benchmarkTiming(fn() => msgpack_unpack($packed), $ITERATIONS);
        echo "    encode: " . number_format($mpEnc['median'], 3) . "ms — " . number_format($mpEnc['ops_per_sec'], 0) . " ops/s\n";
        echo "    decode: " . number_format($mpDec['median'], 3) . "ms — " . number_format($mpDec['ops_per_sec'], 0) . " ops/s\n";
    }
} else {
    echo "  ext-msgpack NOT installed — skipping (pecl install msgpack)\n";
}
echo "\n";

# ============================================================================
# 5. SUMMARY TABLES (LaTeX-friendly)
# ============================================================================

echo "╔══════════════════════════════════════════════════════════════════════════════╗\n";
echo "║  SUMMARY TABLES                                                            ║\n";
echo "╚══════════════════════════════════════════════════════════════════════════════╝\n\n";

# Payload Size table
echo "Payload Size (Bytes):\n";
echo str_pad('Dataset', 20) . str_pad('JSON', 12, ' ', STR_PAD_LEFT) . str_pad('SCON', 12, ' ', STR_PAD_LEFT)
   . str_pad('SCON(min)', 12, ' ', STR_PAD_LEFT) . str_pad('JSON+Gz', 12, ' ', STR_PAD_LEFT)
   . str_pad('SCON+Gz', 12, ' ', STR_PAD_LEFT) . str_pad('Saving', 10, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 90) . "\n";
foreach ($results as $r) {
    $p = $r['payload'];
    $saving = number_format((1 - $p['scon']['raw'] / $p['json']['raw']) * 100, 0) . '%';
    echo str_pad($r['dataset'], 20)
       . str_pad(number_format($p['json']['raw']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['scon']['raw']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['scon_min']['raw']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['json']['gzip']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['scon']['gzip']), 12, ' ', STR_PAD_LEFT)
       . str_pad($saving, 10, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nParsing Time — median ms (p95 / p99):\n";
echo str_pad('Dataset', 20) . str_pad('json_decode', 22, ' ', STR_PAD_LEFT)
   . str_pad('SCON::decode', 22, ' ', STR_PAD_LEFT) . str_pad('Ratio', 10, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 74) . "\n";
foreach ($results as $r) {
    $jd = $r['decode']['json'];
    $sd = $r['decode']['scon'];
    $ratio = $jd['median'] > 0 ? number_format($sd['median'] / $jd['median'], 1) . 'x' : 'N/A';
    echo str_pad($r['dataset'], 20)
       . str_pad(number_format($jd['median'], 2) . " (" . number_format($jd['p95'], 2) . "/" . number_format($jd['p99'], 2) . ")", 22, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sd['median'], 2) . " (" . number_format($sd['p95'], 2) . "/" . number_format($sd['p99'], 2) . ")", 22, ' ', STR_PAD_LEFT)
       . str_pad($ratio, 10, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nEncoding Time — median ms (p95 / p99):\n";
echo str_pad('Dataset', 20) . str_pad('json_encode', 22, ' ', STR_PAD_LEFT)
   . str_pad('SCON::encode', 22, ' ', STR_PAD_LEFT) . str_pad('Ratio', 10, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 74) . "\n";
foreach ($results as $r) {
    $je = $r['encode']['json'];
    $se = $r['encode']['scon'];
    $ratio = $je['median'] > 0 ? number_format($se['median'] / $je['median'], 1) . 'x' : 'N/A';
    echo str_pad($r['dataset'], 20)
       . str_pad(number_format($je['median'], 2) . " (" . number_format($je['p95'], 2) . "/" . number_format($je['p99'], 2) . ")", 22, ' ', STR_PAD_LEFT)
       . str_pad(number_format($se['median'], 2) . " (" . number_format($se['p95'], 2) . "/" . number_format($se['p99'], 2) . ")", 22, ' ', STR_PAD_LEFT)
       . str_pad($ratio, 10, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nDeduplication (autoExtract):\n";
echo str_pad('Dataset', 20) . str_pad('SCON plain', 12, ' ', STR_PAD_LEFT)
   . str_pad('SCON dedup', 12, ' ', STR_PAD_LEFT) . str_pad('Reduction', 12, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 56) . "\n";
foreach ($results as $r) {
    $d = $r['dedup_ratio'];
    echo str_pad($r['dataset'], 20)
       . str_pad(number_format($d['scon_plain']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($d['scon_dedup']), 12, ' ', STR_PAD_LEFT)
       . str_pad($d['reduction_pct'] . '%', 12, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nMemory (Peak RSS delta):\n";
echo str_pad('Dataset', 20) . str_pad('json_dec', 12, ' ', STR_PAD_LEFT) . str_pad('scon_dec', 12, ' ', STR_PAD_LEFT)
   . str_pad('json_enc', 12, ' ', STR_PAD_LEFT) . str_pad('scon_enc', 12, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 68) . "\n";
foreach ($results as $r) {
    $m = $r['memory'];
    echo str_pad($r['dataset'], 20)
       . str_pad(formatBytes($m['json_decode']), 12, ' ', STR_PAD_LEFT)
       . str_pad(formatBytes($m['scon_decode']), 12, ' ', STR_PAD_LEFT)
       . str_pad(formatBytes($m['json_encode']), 12, ' ', STR_PAD_LEFT)
       . str_pad(formatBytes($m['scon_encode']), 12, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nThroughput (MB/s):\n";
echo str_pad('Dataset', 20) . str_pad('json_dec', 12, ' ', STR_PAD_LEFT) . str_pad('scon_dec', 12, ' ', STR_PAD_LEFT)
   . str_pad('json_enc', 12, ' ', STR_PAD_LEFT) . str_pad('scon_enc', 12, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 68) . "\n";
foreach ($results as $r) {
    $t = $r['throughput'];
    echo str_pad($r['dataset'], 20)
       . str_pad(number_format($t['json_decode_mbs'], 2), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['scon_decode_mbs'], 2), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['json_encode_mbs'], 2), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['scon_encode_mbs'], 2), 12, ' ', STR_PAD_LEFT) . "\n";
}

# ============================================================================
# 6. JSON output
# ============================================================================

if ($OUTPUT_MODE === 'json' || $OUTPUT_MODE === 'both') {
    $jsonOutput = json_encode([
        'meta' => [
            'lang' => 'php',
            'suite' => 'standard',
            'php_version' => PHP_VERSION,
            'iterations' => $ITERATIONS,
            'date' => date('Y-m-d\TH:i:s'),
            'timestamp' => time(),
            'hostname' => gethostname(),
            'msgpack_available' => function_exists('msgpack_pack'),
        ],
        'results' => $results,
    ], JSON_PRETTY_PRINT);

    $outDir = __DIR__ . '/datasets';
    if (!is_dir($outDir)) mkdir($outDir, 0755, true);
    $outPath = $outDir . '/php_' . date('Ymd_His') . '.json';
    file_put_contents($outPath, $jsonOutput);
    echo "\nJSON results saved to: {$outPath}\n";
}

echo "\nDone.\n";
