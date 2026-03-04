#!/usr/bin/env php
<?php
# bench/bench_ext.php
# SCON Benchmark — Native Extension (Rust via ext-php-rs)
# Usage: php -d extension=./target/release/libscon_php.so bench/bench_ext.php [--iterations=100]
#
# Compares: json_encode/decode (C) vs scon_encode/decode (Rust native)
# Also loads PHP userland for 3-way comparison if available

if (!function_exists('scon_encode')) {
    echo "ERROR: scon-php extension not loaded.\n";
    echo "Build:  cd /var/www/scon && cargo build --release -p scon-php\n";
    echo "Run:    php -d extension=./target/release/libscon_php.so bench/bench_ext.php\n";
    exit(1);
}

# Load PHP userland for 3-way comparison
$hasUserland = false;
$baseDir = dirname(__DIR__) . '/php';
if (file_exists("$baseDir/Scon.php")) {
    require_once "$baseDir/bootstrap.php";
    require_once "$baseDir/Scon.php";
    require_once "$baseDir/Encoder.php";
    require_once "$baseDir/Decoder.php";
    require_once "$baseDir/Minifier.php";
    require_once "$baseDir/SchemaRegistry.php";
    require_once "$baseDir/Validator.php";
    require_once "$baseDir/TreeHash.php";
    $hasUserland = true;
}

$args = getopt('', ['iterations:', 'help']);
if (isset($args['help'])) {
    echo "Usage: php -d extension=libscon_php.so bench/bench_ext.php [--iterations=100]\n";
    exit(0);
}
$ITERATIONS = (int)($args['iterations'] ?? 100);

echo "╔══════════════════════════════════════════════════════════════╗\n";
echo "║  SCON Benchmark — Native Extension (PHP " . PHP_VERSION . ")       ║\n";
echo "║  Iterations: {$ITERATIONS}" . str_repeat(' ', 46 - strlen((string)$ITERATIONS)) . "║\n";
echo "║  Userland comparison: " . ($hasUserland ? 'YES' : 'NO ') . "                                   ║\n";
echo "╚══════════════════════════════════════════════════════════════╝\n\n";

# ============================================================================
# DATASET GENERATION (same as bench.php)
# ============================================================================

function generateDatasets(): array {
    $datasets = [];
    srand(42);

    # Dataset 1: OpenAPI Specs
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
                    '400' => ['description' => 'Bad Request'],
                    '401' => ['description' => 'Unauthorized'],
                    '404' => ['description' => 'Not Found'],
                    '500' => ['description' => 'Internal Server Error'],
                ],
            ]];
        }
    }
    $datasets['OpenAPI Specs'] = $spec;

    # Dataset 2: Config Records
    $services = ['auth', 'billing', 'notifications', 'analytics', 'search', 'storage', 'cache', 'queue', 'email', 'sms'];
    $envs = ['production', 'staging', 'development', 'testing'];
    $configRecords = ['services' => [], 'feature_flags' => []];
    foreach ($services as $svc) {
        foreach ($envs as $env) {
            $configRecords['services'][] = [
                'service_name' => $svc, 'environment' => $env,
                'host' => "svc-{$svc}.{$env}.internal", 'port' => rand(3000, 9000),
                'replicas' => rand(1, 8), 'health_check' => "/health", 'timeout_ms' => rand(1000, 30000),
                'retry_policy' => ['max_retries' => rand(1, 5), 'backoff_ms' => rand(100, 2000)],
                'tls' => $env === 'production', 'log_level' => $env === 'production' ? 'ERROR' : 'DEBUG',
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
            'enabled' => (bool)rand(0, 1), 'rollout_percentage' => rand(0, 100),
            'targeting_rules' => [
                ['attribute' => 'plan', 'operator' => 'in', 'values' => ['pro', 'enterprise']],
                ['attribute' => 'country', 'operator' => 'eq', 'value' => ['CL', 'US', 'MX', 'AR'][rand(0, 3)]],
            ],
            'created_at' => '2025-' . str_pad(rand(1, 12), 2, '0', STR_PAD_LEFT) . '-' . str_pad(rand(1, 28), 2, '0', STR_PAD_LEFT),
            'updated_at' => '2026-03-01T12:00:00',
        ];
    }
    $datasets['Config Records'] = $configRecords;

    # Dataset 3: DB Exports
    $types = ['INT UNSIGNED', 'BIGINT UNSIGNED', 'VARCHAR(255)', 'VARCHAR(2000)', 'TEXT', 'DATETIME', 'TINYINT(1)', 'DECIMAL(12,2)'];
    $tableNames = ['users', 'profiles', 'entities', 'entity_relationships', 'orders', 'order_items', 'products', 'categories',
        'payments', 'payment_methods', 'invoices', 'workorders', 'work_units', 'notes', 'files', 'audit_log',
        'roles', 'role_packages', 'permissions', 'sessions', 'settings', 'notifications', 'email_queue', 'data_values_history'];
    $colNames = ['scope_entity_id', 'name', 'status', 'type', 'code', 'description', 'amount', 'total_clp',
        'created_at', 'updated_at', 'created_by', 'email', 'phone', 'parent_id', 'sort_order'];
    $tables = [];
    foreach ($tableNames as $tn) {
        $colCount = rand(5, 15);
        $columns = [['name' => 'id', 'definition' => 'INT UNSIGNED AUTO_INCREMENT PRIMARY KEY']];
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

# ============================================================================
# BENCHMARK HELPERS
# ============================================================================

function formatBytes(int $bytes): string {
    if ($bytes >= 1048576) return number_format($bytes / 1048576, 2) . ' MB';
    if ($bytes >= 1024) return number_format($bytes / 1024, 1) . ' KB';
    return number_format($bytes) . ' B';
}

function pct(int $base, int $val): string {
    if ($base === 0) return 'N/A';
    $p = (($val - $base) / $base) * 100;
    return ($p < 0 ? '' : '+') . number_format($p, 1) . '%';
}

function bench(callable $fn, int $iters): array {
    for ($i = 0; $i < min(5, $iters); $i++) $fn();
    $times = [];
    for ($i = 0; $i < $iters; $i++) {
        $s = hrtime(true);
        $fn();
        $times[] = (hrtime(true) - $s) / 1e6;
    }
    sort($times);
    $n = count($times);
    return [
        'median' => $times[(int)($n * 0.5)],
        'p95' => $times[(int)($n * 0.95)],
        'p99' => $times[(int)($n * 0.99)],
        'mean' => array_sum($times) / $n,
        'min' => $times[0],
        'max' => $times[$n - 1],
        'ops_s' => $n / (array_sum($times) / 1000),
    ];
}

function benchMem(callable $fn): int {
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
# RUN BENCHMARKS
# ============================================================================

# Load from canonical fixtures (same as Rust bench) or generate fallback
$fixtureDir = __DIR__ . '/fixtures';
$fixtureMap = [
    'OpenAPI Specs'  => 'openapi_specs.json',
    'Config Records' => 'config_records.json',
    'DB Exports'     => 'db_exports.json',
];

$useFixtures = is_dir($fixtureDir);
if ($useFixtures) {
    echo "Loading fixtures from bench/fixtures/...\n";
    $datasets = [];
    foreach ($fixtureMap as $name => $file) {
        $path = "$fixtureDir/$file";
        if (!file_exists($path)) { echo "  MISSING: $path\n"; $useFixtures = false; break; }
        $datasets[$name] = json_decode(file_get_contents($path), true);
        echo "  {$name}: " . formatBytes(strlen(file_get_contents($path))) . " JSON\n";
    }
}
if (!$useFixtures) {
    echo "Generating datasets (fixtures not found)...\n";
    $datasets = generateDatasets();
    foreach ($datasets as $name => $data) {
        echo "  {$name}: " . formatBytes(strlen(json_encode($data))) . " JSON\n";
    }
}
echo "\n";

$results = [];

foreach ($datasets as $datasetName => $data) {
    echo "━━━ {$datasetName} ━━━\n";
    $r = ['dataset' => $datasetName];

    # --- Payload Size ---
    $jsonStr = json_encode($data);
    $jsonSize = strlen($jsonStr);

    $sconNativeStr = scon_encode($data);
    $sconNativeSize = strlen($sconNativeStr);

    $sconMinStr = scon_minify($sconNativeStr);
    $sconMinSize = strlen($sconMinStr);

    $r['payload'] = [
        'json' => ['raw' => $jsonSize, 'gzip' => strlen(gzencode($jsonStr, 9))],
        'scon_native' => ['raw' => $sconNativeSize, 'gzip' => strlen(gzencode($sconNativeStr, 9))],
        'scon_native_min' => ['raw' => $sconMinSize, 'gzip' => strlen(gzencode($sconMinStr, 9))],
    ];

    echo "  Payload Size:\n";
    echo "    JSON:           " . formatBytes($jsonSize) . "\n";
    echo "    SCON(native):   " . formatBytes($sconNativeSize) . " (" . pct($jsonSize, $sconNativeSize) . ")\n";
    echo "    SCON(nat+min):  " . formatBytes($sconMinSize) . " (" . pct($jsonSize, $sconMinSize) . ")\n";

    # --- Encoding Time: 3-way comparison ---
    echo "  Encoding ({$ITERATIONS} iters):\n";

    $jeT = bench(fn() => json_encode($data), $ITERATIONS);
    echo "    json_encode:     " . number_format($jeT['median'], 3) . "ms  p95:" . number_format($jeT['p95'], 3) . "  " . number_format($jeT['ops_s'], 0) . " ops/s\n";

    $seNT = bench(fn() => scon_encode($data), $ITERATIONS);
    echo "    scon_encode(rs): " . number_format($seNT['median'], 3) . "ms  p95:" . number_format($seNT['p95'], 3) . "  " . number_format($seNT['ops_s'], 0) . " ops/s\n";

    $encRatio = $jeT['median'] > 0 ? number_format($seNT['median'] / $jeT['median'], 1) . 'x' : '-';
    echo "    Ratio native/json: {$encRatio}\n";

    $r['encode'] = ['json' => $jeT, 'scon_native' => $seNT];

    if ($hasUserland) {
        $seUT = bench(fn() => \bX\Scon\Scon::encode($data), $ITERATIONS);
        echo "    SCON::encode(php):" . number_format($seUT['median'], 3) . "ms  " . number_format($seUT['ops_s'], 0) . " ops/s\n";
        $speedup = $seUT['median'] > 0 ? number_format($seUT['median'] / $seNT['median'], 1) . 'x' : '-';
        echo "    Speedup native vs userland: {$speedup}\n";
        $r['encode']['scon_userland'] = $seUT;
    }

    # --- Decoding Time ---
    echo "  Decoding ({$ITERATIONS} iters):\n";

    $jdT = bench(fn() => json_decode($jsonStr, true), $ITERATIONS);
    echo "    json_decode:     " . number_format($jdT['median'], 3) . "ms  p95:" . number_format($jdT['p95'], 3) . "  " . number_format($jdT['ops_s'], 0) . " ops/s\n";

    $sdNT = bench(fn() => scon_decode($sconNativeStr), $ITERATIONS);
    echo "    scon_decode(rs): " . number_format($sdNT['median'], 3) . "ms  p95:" . number_format($sdNT['p95'], 3) . "  " . number_format($sdNT['ops_s'], 0) . " ops/s\n";

    $decRatio = $jdT['median'] > 0 ? number_format($sdNT['median'] / $jdT['median'], 1) . 'x' : '-';
    echo "    Ratio native/json: {$decRatio}\n";

    $r['decode'] = ['json' => $jdT, 'scon_native' => $sdNT];

    if ($hasUserland) {
        $sdUT = bench(fn() => \bX\Scon\Scon::decode($sconNativeStr), $ITERATIONS);
        echo "    SCON::decode(php):" . number_format($sdUT['median'], 3) . "ms  " . number_format($sdUT['ops_s'], 0) . " ops/s\n";
        $speedup = $sdUT['median'] > 0 ? number_format($sdUT['median'] / $sdNT['median'], 1) . 'x' : '-';
        echo "    Speedup native vs userland: {$speedup}\n";
        $r['decode']['scon_userland'] = $sdUT;
    }

    # --- Minify/Expand ---
    echo "  Minify/Expand ({$ITERATIONS} iters):\n";
    $minT = bench(fn() => scon_minify($sconNativeStr), $ITERATIONS);
    echo "    minify(native):  " . number_format($minT['median'], 3) . "ms  " . number_format($minT['ops_s'], 0) . " ops/s\n";
    $expT = bench(fn() => scon_expand($sconMinStr), $ITERATIONS);
    echo "    expand(native):  " . number_format($expT['median'], 3) . "ms  " . number_format($expT['ops_s'], 0) . " ops/s\n";
    $r['minify'] = $minT;
    $r['expand'] = $expT;

    # --- Memory ---
    echo "  Memory (heap delta):\n";
    $jdM = benchMem(fn() => json_decode($jsonStr, true));
    echo "    json_decode:     " . formatBytes($jdM) . "\n";
    $sdM = benchMem(fn() => scon_decode($sconNativeStr));
    echo "    scon_decode(rs): " . formatBytes($sdM) . "\n";
    $r['memory'] = ['json_decode' => $jdM, 'scon_decode_native' => $sdM];

    # --- Throughput ---
    $jdMBs = ($jsonSize / 1048576) * $jdT['ops_s'];
    $sdMBs = ($sconNativeSize / 1048576) * $sdNT['ops_s'];
    $jeMBs = ($jsonSize / 1048576) * $jeT['ops_s'];
    $seMBs = ($sconNativeSize / 1048576) * $seNT['ops_s'];
    echo "  Throughput:\n";
    echo "    json_decode:     " . number_format($jdMBs, 1) . " MB/s\n";
    echo "    scon_decode(rs): " . number_format($sdMBs, 1) . " MB/s\n";
    echo "    json_encode:     " . number_format($jeMBs, 1) . " MB/s\n";
    echo "    scon_encode(rs): " . number_format($seMBs, 1) . " MB/s\n";
    $r['throughput'] = compact('jdMBs', 'sdMBs', 'jeMBs', 'seMBs');

    # --- Roundtrip verification ---
    $decoded = scon_decode(scon_encode($data));
    $roundtrip = ($decoded == $data) ? 'OK' : 'FAIL';
    echo "  Roundtrip: {$roundtrip}\n";
    $r['roundtrip'] = $roundtrip;

    $results[] = $r;
    echo "\n";
}

# ============================================================================
# SUMMARY TABLE
# ============================================================================

echo "╔══════════════════════════════════════════════════════════════════════════════════════╗\n";
echo "║  SUMMARY — 3-Way Comparison: json(C) vs scon(Rust native) vs scon(PHP userland)    ║\n";
echo "╚══════════════════════════════════════════════════════════════════════════════════════╝\n\n";

echo "Encoding — median ms:\n";
$hdr = str_pad('Dataset', 20) . str_pad('json(C)', 12, ' ', STR_PAD_LEFT)
     . str_pad('scon(Rust)', 12, ' ', STR_PAD_LEFT)
     . str_pad('Ratio', 8, ' ', STR_PAD_LEFT);
if ($hasUserland) $hdr .= str_pad('scon(PHP)', 12, ' ', STR_PAD_LEFT) . str_pad('Speedup', 10, ' ', STR_PAD_LEFT);
echo $hdr . "\n" . str_repeat('─', strlen($hdr)) . "\n";
foreach ($results as $r) {
    $je = $r['encode']['json']['median'];
    $se = $r['encode']['scon_native']['median'];
    $ratio = $je > 0 ? number_format($se / $je, 1) . 'x' : '-';
    $line = str_pad($r['dataset'], 20) . str_pad(number_format($je, 3), 12, ' ', STR_PAD_LEFT)
          . str_pad(number_format($se, 3), 12, ' ', STR_PAD_LEFT) . str_pad($ratio, 8, ' ', STR_PAD_LEFT);
    if ($hasUserland && isset($r['encode']['scon_userland'])) {
        $su = $r['encode']['scon_userland']['median'];
        $speedup = $se > 0 ? number_format($su / $se, 1) . 'x' : '-';
        $line .= str_pad(number_format($su, 3), 12, ' ', STR_PAD_LEFT) . str_pad($speedup, 10, ' ', STR_PAD_LEFT);
    }
    echo $line . "\n";
}

echo "\nDecoding — median ms:\n";
echo $hdr . "\n" . str_repeat('─', strlen($hdr)) . "\n";
foreach ($results as $r) {
    $jd = $r['decode']['json']['median'];
    $sd = $r['decode']['scon_native']['median'];
    $ratio = $jd > 0 ? number_format($sd / $jd, 1) . 'x' : '-';
    $line = str_pad($r['dataset'], 20) . str_pad(number_format($jd, 3), 12, ' ', STR_PAD_LEFT)
          . str_pad(number_format($sd, 3), 12, ' ', STR_PAD_LEFT) . str_pad($ratio, 8, ' ', STR_PAD_LEFT);
    if ($hasUserland && isset($r['decode']['scon_userland'])) {
        $su = $r['decode']['scon_userland']['median'];
        $speedup = $sd > 0 ? number_format($su / $sd, 1) . 'x' : '-';
        $line .= str_pad(number_format($su, 3), 12, ' ', STR_PAD_LEFT) . str_pad($speedup, 10, ' ', STR_PAD_LEFT);
    }
    echo $line . "\n";
}

# ============================================================================
# JSON OUTPUT
# ============================================================================

$outDir = __DIR__ . '/datasets';
if (!is_dir($outDir)) mkdir($outDir, 0755, true);
$outPath = $outDir . '/php_ext_' . date('Ymd_His') . '.json';
file_put_contents($outPath, json_encode([
    'meta' => [
        'lang' => 'php_ext',
        'suite' => 'native_extension',
        'php_version' => PHP_VERSION,
        'iterations' => $ITERATIONS,
        'date' => date('Y-m-d\TH:i:s'),
        'timestamp' => time(),
        'hostname' => gethostname(),
        'has_userland' => $hasUserland,
    ],
    'results' => $results,
], JSON_PRETTY_PRINT));
echo "\nJSON results: {$outPath}\n";
echo "Done.\n";
