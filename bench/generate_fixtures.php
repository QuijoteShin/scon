#!/usr/bin/env php
<?php
# bench/generate_fixtures.php
# Generates canonical fixture files for cross-language benchmarks.
# Run: php bench/generate_fixtures.php
# Output: bench/fixtures/{openapi_specs,config_records,db_exports}.json
#
# These fixtures are byte-identical inputs for PHP, JS, and Rust benchmarks.
# Commit the generated files — they only change when the generation algorithm changes.

# ============================================================================
# Dataset generators (extracted from bench.php, srand(42) deterministic)
# ============================================================================

function generateDatasets(): array {
    $datasets = [];
    srand(42); # Deterministic for reproducible benchmarks

    # --- Dataset 1: OpenAPI Specs ---
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

    # --- Dataset 2: Config Records ---
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

    # --- Dataset 3: DB Exports ---
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

# ============================================================================
# Generate and write fixtures
# ============================================================================

$fixtureDir = __DIR__ . '/fixtures';
if (!is_dir($fixtureDir)) mkdir($fixtureDir, 0755, true);

$slugMap = [
    'OpenAPI Specs'  => 'openapi_specs',
    'Config Records' => 'config_records',
    'DB Exports'     => 'db_exports',
];

echo "Generating canonical fixtures (srand(42))...\n\n";

$datasets = generateDatasets();
foreach ($datasets as $name => $data) {
    $slug = $slugMap[$name];
    $json = json_encode($data, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    $path = "$fixtureDir/{$slug}.json";
    file_put_contents($path, $json);
    $bytes = strlen($json);
    $kb = number_format($bytes / 1024, 1);
    echo "  {$slug}.json: {$bytes} bytes ({$kb} KB)\n";
}

echo "\nFixtures written to bench/fixtures/\n";
echo "These files should be committed to git.\n";
