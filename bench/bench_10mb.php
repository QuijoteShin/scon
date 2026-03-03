#!/usr/bin/env php
<?php
# bench/bench_10mb.php
# SCON Benchmark — 10MB Datasets (Server Logs, Geo API, Multimedia Metadata)
# Para paper académico. Incluye memory real y distribución de latencias.
#
# Usage: php bench/bench_10mb.php [--iterations=30]

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

$ITERATIONS = (int)(getopt('', ['iterations:'])['iterations'] ?? 30);

echo "╔══════════════════════════════════════════════════════════╗\n";
echo "║  SCON Benchmark — 10MB Datasets (PHP " . PHP_VERSION . ")     ║\n";
echo "║  Iterations: {$ITERATIONS}" . str_repeat(' ', 42 - strlen((string)$ITERATIONS)) . "║\n";
echo "╚══════════════════════════════════════════════════════════╝\n\n";

# ============================================================================
# DATASET GENERATORS — ~10MB each
# ============================================================================

function generateServerLogs(int $targetBytes = 10_000_000): array {
    # Structured server logs — alta repetición en campos (level, service, host)
    # Ideal para dedup: mismos keys, mismos values de enums
    $levels = ['INFO', 'WARN', 'ERROR', 'DEBUG', 'TRACE'];
    $services = ['api-gateway', 'auth-service', 'billing', 'notifications', 'search-engine',
                 'cache-proxy', 'queue-worker', 'scheduler', 'storage-api', 'analytics'];
    $hosts = ['node-01.us-east', 'node-02.us-east', 'node-03.eu-west', 'node-04.eu-west',
              'node-05.ap-south', 'node-06.ap-south'];
    $methods = ['GET', 'POST', 'PUT', 'DELETE', 'PATCH'];
    $paths = ['/api/users', '/api/orders', '/api/products', '/api/auth/login', '/api/auth/refresh',
              '/api/search', '/api/billing/invoice', '/api/notifications/send', '/api/health', '/api/metrics'];
    $statusCodes = [200, 201, 204, 301, 400, 401, 403, 404, 500, 502, 503];
    $userAgents = [
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15',
        'curl/7.88.1', 'PostmanRuntime/7.32.3', 'Python-httpx/0.24.1',
    ];

    $logs = [];
    $baseTs = 1740873600 - 86400; # Fixed epoch for reproducibility (2025-03-02 00:00:00 UTC)
    $i = 0;
    while (true) {
        $entry = [
            'timestamp' => date('Y-m-d\TH:i:s.', $baseTs + $i) . str_pad(rand(0, 999), 3, '0', STR_PAD_LEFT) . 'Z',
            'level' => $levels[array_rand($levels)],
            'service' => $services[array_rand($services)],
            'host' => $hosts[array_rand($hosts)],
            'request_id' => sprintf('%08x-%04x-%04x-%04x-%012x', rand(0, 0xFFFFFFFF), rand(0, 0xFFFF), rand(0, 0xFFFF), rand(0, 0xFFFF), rand(0, 0xFFFFFFFFFFFF)),
            'method' => $methods[array_rand($methods)],
            'path' => $paths[array_rand($paths)],
            'status_code' => $statusCodes[array_rand($statusCodes)],
            'duration_ms' => round(rand(1, 5000) / 10, 1),
            'bytes_sent' => rand(100, 500000),
            'user_agent' => $userAgents[array_rand($userAgents)],
            'ip' => rand(1, 254) . '.' . rand(0, 255) . '.' . rand(0, 255) . '.' . rand(1, 254),
            'trace_context' => [
                'trace_id' => sprintf('%08x%08x%08x%08x', rand(0, 0xFFFFFFFF), rand(0, 0xFFFFFFFF), rand(0, 0xFFFFFFFF), rand(0, 0xFFFFFFFF)),
                'span_id' => sprintf('%08x%08x', rand(0, 0xFFFFFFFF), rand(0, 0xFFFFFFFF)),
                'parent_span_id' => rand(0, 1) ? sprintf('%08x%08x', rand(0, 0xFFFFFFFF), rand(0, 0xFFFFFFFF)) : null,
            ],
        ];
        # Occasional extra fields
        if ($entry['level'] === 'ERROR') {
            $entry['error'] = [
                'type' => ['NullPointerException', 'TimeoutException', 'ConnectionRefused', 'AuthExpired', 'RateLimited'][rand(0, 4)],
                'message' => 'Simulated error message for benchmark entry ' . $i,
                'stack_trace' => "at Service.handle(Service.php:" . rand(10, 500) . ")\nat Router.dispatch(Router.php:" . rand(10, 200) . ")",
            ];
        }
        $logs[] = $entry;
        $i++;
        if ($i % 500 === 0 && strlen(json_encode($logs)) >= $targetBytes) break;
        if ($i > 100000) break; # safety
    }
    return ['log_format' => 'structured/v2', 'entries' => $logs];
}

function generateGeoApi(int $targetBytes = 10_000_000): array {
    # Geographic API data — alta repetición en country codes, region types, feature types
    $countries = ['CL', 'AR', 'BR', 'MX', 'CO', 'PE', 'US', 'CA', 'DE', 'FR', 'ES', 'IT', 'JP', 'KR', 'AU'];
    $featureTypes = ['Point', 'LineString', 'Polygon', 'MultiPoint', 'MultiPolygon'];
    $categories = ['restaurant', 'hotel', 'hospital', 'school', 'park', 'museum', 'airport', 'station', 'pharmacy', 'bank'];
    $currencies = ['CLP', 'ARS', 'BRL', 'MXN', 'COP', 'PEN', 'USD', 'CAD', 'EUR', 'JPY'];

    $features = [];
    $i = 0;
    while (true) {
        $country = $countries[array_rand($countries)];
        $lat = rand(-560000, 700000) / 10000;
        $lon = rand(-1800000, 1800000) / 10000;
        $feature = [
            'type' => 'Feature',
            'id' => 'geo_' . str_pad($i, 7, '0', STR_PAD_LEFT),
            'geometry' => [
                'type' => $featureTypes[array_rand($featureTypes)],
                'coordinates' => [$lon, $lat],
            ],
            'properties' => [
                'name' => 'Location ' . $i . ' in ' . $country,
                'country' => $country,
                'category' => $categories[array_rand($categories)],
                'rating' => round(rand(10, 50) / 10, 1),
                'reviews_count' => rand(0, 5000),
                'price_range' => rand(1, 4),
                'currency' => $currencies[array_rand($currencies)],
                'is_open' => (bool)rand(0, 1),
                'operating_hours' => [
                    'monday' => ['09:00', '18:00'],
                    'tuesday' => ['09:00', '18:00'],
                    'wednesday' => ['09:00', '18:00'],
                    'thursday' => ['09:00', '18:00'],
                    'friday' => ['09:00', '20:00'],
                    'saturday' => ['10:00', '16:00'],
                    'sunday' => null,
                ],
                'tags' => array_slice(['wifi', 'parking', 'accessible', 'pet-friendly', 'outdoor', 'delivery', 'takeout', 'reservation'], 0, rand(1, 5)),
                'contact' => [
                    'phone' => '+' . rand(1, 99) . ' ' . rand(100, 999) . ' ' . rand(1000, 9999),
                    'email' => 'info@location' . $i . '.example.com',
                    'website' => 'https://location' . $i . '.example.com',
                ],
                'address' => [
                    'street' => rand(1, 9999) . ' Main Street',
                    'city' => 'City ' . rand(1, 500),
                    'region' => 'Region ' . rand(1, 50),
                    'postal_code' => str_pad(rand(10000, 99999), 5, '0', STR_PAD_LEFT),
                    'country' => $country,
                ],
            ],
        ];
        $features[] = $feature;
        $i++;
        if ($i % 200 === 0 && strlen(json_encode($features)) >= $targetBytes) break;
        if ($i > 50000) break;
    }
    return [
        'type' => 'FeatureCollection',
        'metadata' => ['generated' => date('c'), 'count' => count($features), 'bbox' => [-180, -56, 180, 70]],
        'features' => $features,
    ];
}

function generateMultimediaMetadata(int $targetBytes = 10_000_000): array {
    # Multimedia metadata — alta repetición en codecs, resolutions, formats
    $codecs = ['H.264', 'H.265/HEVC', 'VP9', 'AV1', 'AAC', 'Opus', 'FLAC', 'MP3'];
    $videoRes = ['1920x1080', '3840x2160', '1280x720', '2560x1440', '7680x4320'];
    $containers = ['mp4', 'mkv', 'webm', 'avi', 'mov', 'flac', 'ogg', 'wav'];
    $colorSpaces = ['sRGB', 'DCI-P3', 'Rec.709', 'Rec.2020', 'Adobe RGB'];
    $contentTypes = ['movie', 'series', 'documentary', 'music_video', 'podcast', 'audiobook', 'concert'];

    $items = [];
    $i = 0;
    while (true) {
        $isVideo = rand(0, 1);
        $item = [
            'asset_id' => 'media_' . str_pad($i, 7, '0', STR_PAD_LEFT),
            'title' => 'Media Asset ' . $i,
            'content_type' => $contentTypes[array_rand($contentTypes)],
            'duration_seconds' => rand(30, 14400),
            'file_size_bytes' => rand(1000000, 50000000000),
            'container' => $containers[array_rand($containers)],
            'created_at' => date('Y-m-d\TH:i:s', 1740873600 - rand(0, 86400 * 365 * 5)) . 'Z',
            'streams' => [],
            'metadata' => [
                'encoder' => ['FFmpeg', 'HandBrake', 'Adobe Media Encoder', 'DaVinci Resolve'][rand(0, 3)],
                'encoding_date' => date('Y-m-d', 1740873600 - rand(0, 86400 * 365)),
                'language' => ['en', 'es', 'fr', 'de', 'ja', 'pt', 'ko'][rand(0, 6)],
                'copyright' => '(c) ' . rand(2020, 2026) . ' Example Studios',
            ],
            'thumbnails' => [
                ['url' => 'https://cdn.example.com/thumbs/' . $i . '/small.jpg', 'width' => 160, 'height' => 90],
                ['url' => 'https://cdn.example.com/thumbs/' . $i . '/medium.jpg', 'width' => 320, 'height' => 180],
                ['url' => 'https://cdn.example.com/thumbs/' . $i . '/large.jpg', 'width' => 640, 'height' => 360],
            ],
            'processing' => [
                'status' => ['completed', 'processing', 'queued', 'failed'][rand(0, 3)],
                'progress' => rand(0, 100),
                'variants' => [
                    ['quality' => '1080p', 'bitrate_kbps' => rand(2000, 8000), 'status' => 'completed'],
                    ['quality' => '720p', 'bitrate_kbps' => rand(1000, 4000), 'status' => 'completed'],
                    ['quality' => '480p', 'bitrate_kbps' => rand(500, 2000), 'status' => 'completed'],
                ],
            ],
        ];

        if ($isVideo) {
            $item['streams'][] = [
                'type' => 'video', 'codec' => $codecs[rand(0, 3)], 'resolution' => $videoRes[array_rand($videoRes)],
                'bitrate_kbps' => rand(1000, 50000), 'framerate' => [23.976, 24, 25, 29.97, 30, 59.94, 60][rand(0, 6)],
                'color_space' => $colorSpaces[array_rand($colorSpaces)], 'hdr' => (bool)rand(0, 1),
                'bit_depth' => [8, 10, 12][rand(0, 2)],
            ];
        }
        $item['streams'][] = [
            'type' => 'audio', 'codec' => $codecs[rand(4, 7)],
            'bitrate_kbps' => rand(96, 1411), 'sample_rate' => [44100, 48000, 96000][rand(0, 2)],
            'channels' => [1, 2, 6, 8][rand(0, 3)], 'language' => 'en',
        ];

        $items[] = $item;
        $i++;
        if ($i % 200 === 0 && strlen(json_encode($items)) >= $targetBytes) break;
        if ($i > 50000) break;
    }
    return ['collection' => 'media_assets', 'count' => count($items), 'items' => $items];
}

# ============================================================================
# BENCHMARK RUNNER
# ============================================================================

function bench(callable $fn, int $iters): array {
    for ($i = 0; $i < min(3, $iters); $i++) $fn(); # warmup
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

function memoryOf(callable $fn): int {
    # Warm, then measure actual PHP allocation delta
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

function fmt(int $b): string {
    if ($b >= 1048576) return number_format($b / 1048576, 2) . ' MB';
    if ($b >= 1024) return number_format($b / 1024, 1) . ' KB';
    return number_format($b) . ' B';
}

function pct(int $base, int $val): string {
    if ($base === 0) return 'N/A';
    $p = (($val - $base) / $base) * 100;
    return ($p < 0 ? '' : '+') . number_format($p, 1) . '%';
}

# ============================================================================
# GENERATE & RUN
# ============================================================================

$generators = [
    'Server Logs' => 'generateServerLogs',
    'Geo API' => 'generateGeoApi',
    'Multimedia Metadata' => 'generateMultimediaMetadata',
];

srand(42); # Deterministic for reproducible benchmarks

$allResults = [];

foreach ($generators as $name => $genFn) {
    echo "Generating {$name} (~10MB)...\n";
    $data = $genFn(10_000_000);
    $jsonStr = json_encode($data);
    $jsonSize = strlen($jsonStr);
    echo "  Generated: " . fmt($jsonSize) . " JSON\n\n";

    echo "━━━ {$name} ━━━\n";

    # === Encode ===
    $sconStr = Scon::encode($data);
    $sconSize = strlen($sconStr);

    $sconMinStr = Scon::minify($sconStr);
    $sconMinSize = strlen($sconMinStr);

    $sconDedupStr = Scon::encode($data, ['autoExtract' => true]);
    $sconDedupSize = strlen($sconDedupStr);
    $sconDedupMinStr = Scon::minify($sconDedupStr);
    $sconDedupMinSize = strlen($sconDedupMinStr);

    $jsonGz = strlen(gzencode($jsonStr, 9));
    $sconGz = strlen(gzencode($sconStr, 9));
    $sconMinGz = strlen(gzencode($sconMinStr, 9));
    $sconDedupGz = strlen(gzencode($sconDedupStr, 9));

    echo "  Payload Size:\n";
    echo "    JSON:                " . fmt($jsonSize) . " | gzip: " . fmt($jsonGz) . "\n";
    echo "    SCON:                " . fmt($sconSize) . " (" . pct($jsonSize, $sconSize) . ") | gzip: " . fmt($sconGz) . "\n";
    echo "    SCON(min):           " . fmt($sconMinSize) . " (" . pct($jsonSize, $sconMinSize) . ") | gzip: " . fmt($sconMinGz) . "\n";
    echo "    SCON(dedup):         " . fmt($sconDedupSize) . " (" . pct($jsonSize, $sconDedupSize) . ") | gzip: " . fmt($sconDedupGz) . "\n";
    echo "    SCON(dedup+min):     " . fmt($sconDedupMinSize) . " (" . pct($jsonSize, $sconDedupMinSize) . ")\n";

    $dedupPct = $sconSize > 0 ? round(($sconSize - $sconDedupSize) / $sconSize * 100, 1) : 0;
    echo "    Dedup extra savings: {$dedupPct}%\n";

    # === Timing ===
    echo "  Encoding ({$ITERATIONS} iters):\n";

    $jeT = bench(fn() => json_encode($data), $ITERATIONS);
    echo "    json_encode:   " . number_format($jeT['median'], 1) . "ms  p95:" . number_format($jeT['p95'], 1) . "  p99:" . number_format($jeT['p99'], 1) . "  " . number_format($jeT['ops_s'], 0) . " ops/s\n";

    $seT = bench(fn() => Scon::encode($data), $ITERATIONS);
    echo "    SCON::encode:  " . number_format($seT['median'], 1) . "ms  p95:" . number_format($seT['p95'], 1) . "  p99:" . number_format($seT['p99'], 1) . "  " . number_format($seT['ops_s'], 0) . " ops/s\n";

    $sdT = bench(fn() => Scon::encode($data, ['autoExtract' => true]), $ITERATIONS);
    echo "    SCON(dedup):   " . number_format($sdT['median'], 1) . "ms  p95:" . number_format($sdT['p95'], 1) . "  p99:" . number_format($sdT['p99'], 1) . "  " . number_format($sdT['ops_s'], 0) . " ops/s\n";

    echo "  Decoding ({$ITERATIONS} iters):\n";

    $jdT = bench(fn() => json_decode($jsonStr, true), $ITERATIONS);
    echo "    json_decode:   " . number_format($jdT['median'], 1) . "ms  p95:" . number_format($jdT['p95'], 1) . "  p99:" . number_format($jdT['p99'], 1) . "  " . number_format($jdT['ops_s'], 0) . " ops/s\n";

    $sdecT = bench(fn() => Scon::decode($sconStr), $ITERATIONS);
    echo "    SCON::decode:  " . number_format($sdecT['median'], 1) . "ms  p95:" . number_format($sdecT['p95'], 1) . "  p99:" . number_format($sdecT['p99'], 1) . "  " . number_format($sdecT['ops_s'], 0) . " ops/s\n";

    $smdT = bench(fn() => Scon::decode($sconMinStr), $ITERATIONS);
    echo "    SCON(min)dec:  " . number_format($smdT['median'], 1) . "ms  p95:" . number_format($smdT['p95'], 1) . "  p99:" . number_format($smdT['p99'], 1) . "  " . number_format($smdT['ops_s'], 0) . " ops/s\n";

    echo "  Minify/Expand ({$ITERATIONS} iters):\n";
    $minT = bench(fn() => Scon::minify($sconStr), $ITERATIONS);
    echo "    minify:        " . number_format($minT['median'], 1) . "ms  " . number_format($minT['ops_s'], 0) . " ops/s\n";
    $expT = bench(fn() => Scon::expand($sconMinStr), $ITERATIONS);
    echo "    expand:        " . number_format($expT['median'], 1) . "ms  " . number_format($expT['ops_s'], 0) . " ops/s\n";

    # === Memory via /proc ===
    echo "  Memory (VmRSS delta):\n";
    $jdM = memoryOf(fn() => json_decode($jsonStr, true));
    echo "    json_decode:   " . fmt($jdM) . "\n";
    $sdM = memoryOf(fn() => Scon::decode($sconStr));
    echo "    SCON::decode:  " . fmt($sdM) . "\n";

    # === Throughput ===
    $jdMBs = ($jsonSize / 1048576) * $jdT['ops_s'];
    $sdMBs = ($sconSize / 1048576) * $sdecT['ops_s'];
    $jeMBs = ($jsonSize / 1048576) * $jeT['ops_s'];
    $seMBs = ($sconSize / 1048576) * $seT['ops_s'];
    echo "  Throughput:\n";
    echo "    json_decode:   " . number_format($jdMBs, 1) . " MB/s\n";
    echo "    SCON::decode:  " . number_format($sdMBs, 1) . " MB/s\n";
    echo "    json_encode:   " . number_format($jeMBs, 1) . " MB/s\n";
    echo "    SCON::encode:  " . number_format($seMBs, 1) . " MB/s\n";

    # === MessagePack ===
    if (function_exists('msgpack_pack')) {
        $mp = msgpack_pack($data);
        $mpSize = strlen($mp);
        $mpGz = strlen(gzencode($mp, 9));
        echo "  MessagePack:     " . fmt($mpSize) . " (" . pct($jsonSize, $mpSize) . ") | gzip: " . fmt($mpGz) . "\n";
        $mpEnc = bench(fn() => msgpack_pack($data), $ITERATIONS);
        $mpDec = bench(fn() => msgpack_unpack($mp), $ITERATIONS);
        echo "    encode: " . number_format($mpEnc['median'], 1) . "ms  decode: " . number_format($mpDec['median'], 1) . "ms\n";
    }

    $allResults[] = [
        'dataset' => $name,
        'json_size' => $jsonSize,
        'payload' => compact('jsonSize', 'sconSize', 'sconMinSize', 'sconDedupSize', 'sconDedupMinSize', 'jsonGz', 'sconGz', 'sconMinGz', 'sconDedupGz'),
        'encode' => ['json' => $jeT, 'scon' => $seT, 'scon_dedup' => $sdT],
        'decode' => ['json' => $jdT, 'scon' => $sdecT, 'scon_min' => $smdT],
        'minify' => $minT, 'expand' => $expT,
        'memory' => ['json_decode' => $jdM, 'scon_decode' => $sdM],
        'throughput' => compact('jdMBs', 'sdMBs', 'jeMBs', 'seMBs'),
        'dedup_pct' => $dedupPct,
    ];

    echo "\n";
}

# ============================================================================
# SUMMARY TABLE (paper-ready)
# ============================================================================

echo "╔══════════════════════════════════════════════════════════════════════════════════════╗\n";
echo "║  SUMMARY — 10MB Datasets                                                           ║\n";
echo "╚══════════════════════════════════════════════════════════════════════════════════════╝\n\n";

echo "Payload Size (Bytes):\n";
$hdr = str_pad('Dataset', 22) . str_pad('JSON', 12, ' ', STR_PAD_LEFT) . str_pad('SCON', 12, ' ', STR_PAD_LEFT)
     . str_pad('SCON(min)', 12, ' ', STR_PAD_LEFT) . str_pad('SCON(ded)', 12, ' ', STR_PAD_LEFT)
     . str_pad('JSON+Gz', 10, ' ', STR_PAD_LEFT) . str_pad('SCON+Gz', 10, ' ', STR_PAD_LEFT)
     . str_pad('Save%', 8, ' ', STR_PAD_LEFT);
echo $hdr . "\n" . str_repeat('─', strlen($hdr)) . "\n";
foreach ($allResults as $r) {
    $p = $r['payload'];
    $save = number_format((1 - $p['sconDedupMinSize'] / $p['jsonSize']) * 100, 0);
    echo str_pad($r['dataset'], 22)
       . str_pad(number_format($p['jsonSize']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['sconSize']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['sconMinSize']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['sconDedupSize']), 12, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['jsonGz']), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($p['sconGz']), 10, ' ', STR_PAD_LEFT)
       . str_pad($save . '%', 8, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nLatency Distribution — Decode (ms):\n";
$hdr2 = str_pad('Dataset', 22) . str_pad('json p50', 10, ' ', STR_PAD_LEFT) . str_pad('p95', 10, ' ', STR_PAD_LEFT) . str_pad('p99', 10, ' ', STR_PAD_LEFT)
      . str_pad('scon p50', 10, ' ', STR_PAD_LEFT) . str_pad('p95', 10, ' ', STR_PAD_LEFT) . str_pad('p99', 10, ' ', STR_PAD_LEFT)
      . str_pad('Ratio', 8, ' ', STR_PAD_LEFT);
echo $hdr2 . "\n" . str_repeat('─', strlen($hdr2)) . "\n";
foreach ($allResults as $r) {
    $jd = $r['decode']['json'];
    $sd = $r['decode']['scon'];
    $ratio = $jd['median'] > 0 ? number_format($sd['median'] / $jd['median'], 1) . 'x' : '-';
    echo str_pad($r['dataset'], 22)
       . str_pad(number_format($jd['median'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($jd['p95'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($jd['p99'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sd['median'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sd['p95'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sd['p99'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad($ratio, 8, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nLatency Distribution — Encode (ms):\n";
echo $hdr2 . "\n" . str_repeat('─', strlen($hdr2)) . "\n";
foreach ($allResults as $r) {
    $je = $r['encode']['json'];
    $se = $r['encode']['scon'];
    $ratio = $je['median'] > 0 ? number_format($se['median'] / $je['median'], 1) . 'x' : '-';
    echo str_pad($r['dataset'], 22)
       . str_pad(number_format($je['median'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($je['p95'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($je['p99'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($se['median'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($se['p95'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($se['p99'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad($ratio, 8, ' ', STR_PAD_LEFT) . "\n";
}

echo "\nThroughput (MB/s):\n";
$hdr3 = str_pad('Dataset', 22) . str_pad('json_dec', 10, ' ', STR_PAD_LEFT) . str_pad('scon_dec', 10, ' ', STR_PAD_LEFT)
      . str_pad('json_enc', 10, ' ', STR_PAD_LEFT) . str_pad('scon_enc', 10, ' ', STR_PAD_LEFT);
echo $hdr3 . "\n" . str_repeat('─', strlen($hdr3)) . "\n";
foreach ($allResults as $r) {
    $t = $r['throughput'];
    echo str_pad($r['dataset'], 22)
       . str_pad(number_format($t['jdMBs'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['sdMBs'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['jeMBs'], 1), 10, ' ', STR_PAD_LEFT)
       . str_pad(number_format($t['seMBs'], 1), 10, ' ', STR_PAD_LEFT) . "\n";
}

# Save JSON — incremental filename
$outDir = __DIR__ . '/datasets';
if (!is_dir($outDir)) mkdir($outDir, 0755, true);
$outPath = $outDir . '/php_10mb_' . date('Ymd_His') . '.json';
file_put_contents($outPath, json_encode([
    'meta' => [
        'lang' => 'php',
        'suite' => '10mb',
        'php' => PHP_VERSION,
        'iterations' => $ITERATIONS,
        'date' => date('c'),
        'timestamp' => time(),
        'host' => gethostname(),
    ],
    'results' => $allResults,
], JSON_PRETTY_PRINT));
echo "\nJSON results: {$outPath}\n";
echo "Done.\n";
