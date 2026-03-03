#!/usr/bin/env php
<?php
# bench/bench_telemetry.php
# SCON Benchmark — Mining Telemetry (10K sensor readings)
# Measures: payload size, encode/decode time, gzip CPU, end-to-end by BW
#
# Usage: php bench/bench_telemetry.php [--iterations=100]

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

$args = getopt('', ['iterations:']);
$ITERATIONS = (int)($args['iterations'] ?? 100);

echo "╔══════════════════════════════════════════════════════════╗\n";
echo "║  SCON Telemetry Benchmark — Mining (10K readings)       ║\n";
echo "║  PHP " . PHP_VERSION . str_repeat(' ', 47 - strlen(PHP_VERSION)) . "║\n";
echo "║  Iterations: {$ITERATIONS}" . str_repeat(' ', 42 - strlen((string)$ITERATIONS)) . "║\n";
echo "╚══════════════════════════════════════════════════════════╝\n\n";

# Load fixture
$fixturePath = __DIR__ . '/fixtures/telemetry_mine.json';
if (!file_exists($fixturePath)) {
    echo "Fixture not found. Run: php bench/generate_telemetry_fixture.php\n";
    exit(1);
}
$data = json_decode(file_get_contents($fixturePath), true);
echo "Loaded: " . number_format(count($data['readings'])) . " readings from {$data['site']}\n\n";

function formatBytes(int $bytes): string {
    if ($bytes >= 1048576) return number_format($bytes / 1048576, 2) . ' MB';
    if ($bytes >= 1024) return number_format($bytes / 1024, 1) . ' KB';
    return number_format($bytes) . ' B';
}

function pctChange(int $baseline, int $value): string {
    if ($baseline === 0) return 'N/A';
    $pct = (($value - $baseline) / $baseline) * 100;
    return ($pct < 0 ? '' : '+') . number_format($pct, 1) . '%';
}

function benchTiming(callable $fn, int $iters): array {
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
        'min' => $times[0],
        'median' => $times[(int)($n * 0.5)],
        'p95' => $times[(int)($n * 0.95)],
        'p99' => $times[(int)($n * 0.99)],
        'mean' => array_sum($times) / $n,
        'ops_per_sec' => $n / (array_sum($times) / 1000),
    ];
}

# ============================================================
# 1. PAYLOAD SIZE
# ============================================================
echo "━━━ Payload Size ━━━\n";

$jsonStr = json_encode($data);
$jsonSize = strlen($jsonStr);
$jsonPrettyStr = json_encode($data, JSON_PRETTY_PRINT);
$jsonPrettySize = strlen($jsonPrettyStr);

$sconStr = Scon::encode($data);
$sconSize = strlen($sconStr);

$sconMinStr = Scon::minify($sconStr);
$sconMinSize = strlen($sconMinStr);

$sconDedupStr = Scon::encode($data, ['autoExtract' => true]);
$sconDedupSize = strlen($sconDedupStr);

$sconDedupMinStr = Scon::minify($sconDedupStr);
$sconDedupMinSize = strlen($sconDedupMinStr);

echo "  JSON:              " . formatBytes($jsonSize) . "\n";
echo "  JSON(pretty):      " . formatBytes($jsonPrettySize) . "\n";
echo "  SCON:              " . formatBytes($sconSize) . " (" . pctChange($jsonSize, $sconSize) . ")\n";
echo "  SCON(min):         " . formatBytes($sconMinSize) . " (" . pctChange($jsonSize, $sconMinSize) . ")\n";
echo "  SCON(dedup):       " . formatBytes($sconDedupSize) . " (" . pctChange($jsonSize, $sconDedupSize) . ")\n";
echo "  SCON(dedup+min):   " . formatBytes($sconDedupMinSize) . " (" . pctChange($jsonSize, $sconDedupMinSize) . ")\n\n";

# ============================================================
# 2. ENCODE / DECODE TIME
# ============================================================
echo "━━━ Encode Time ({$ITERATIONS} iters) ━━━\n";

$jsonEnc = benchTiming(fn() => json_encode($data), $ITERATIONS);
echo "  json_encode:       " . number_format($jsonEnc['median'], 3) . "ms (p95: " . number_format($jsonEnc['p95'], 3) . "ms) — " . number_format($jsonEnc['ops_per_sec'], 0) . " ops/s\n";

$sconEnc = benchTiming(fn() => Scon::encode($data), $ITERATIONS);
echo "  SCON::encode:      " . number_format($sconEnc['median'], 3) . "ms (p95: " . number_format($sconEnc['p95'], 3) . "ms) — " . number_format($sconEnc['ops_per_sec'], 0) . " ops/s\n";

$encRatio = $sconEnc['median'] / $jsonEnc['median'];
echo "  Ratio:             " . number_format($encRatio, 1) . "x\n\n";

echo "━━━ Decode Time ({$ITERATIONS} iters) ━━━\n";

$jsonDec = benchTiming(fn() => json_decode($jsonStr, true), $ITERATIONS);
echo "  json_decode:       " . number_format($jsonDec['median'], 3) . "ms (p95: " . number_format($jsonDec['p95'], 3) . "ms) — " . number_format($jsonDec['ops_per_sec'], 0) . " ops/s\n";

$sconDec = benchTiming(fn() => Scon::decode($sconStr), $ITERATIONS);
echo "  SCON::decode:      " . number_format($sconDec['median'], 3) . "ms (p95: " . number_format($sconDec['p95'], 3) . "ms) — " . number_format($sconDec['ops_per_sec'], 0) . " ops/s\n";

$decRatio = $sconDec['median'] / $jsonDec['median'];
echo "  Ratio:             " . number_format($decRatio, 1) . "x\n\n";

# ============================================================
# 3. GZIP CPU COST
# ============================================================
echo "━━━ gzip CPU Cost (500 iters) ━━━\n";

$gzIters = 500;

$gzJson = benchTiming(fn() => gzencode($jsonStr, 6), $gzIters);
$gzJsonOut = strlen(gzencode($jsonStr, 6));
echo "  gzip(JSON):        " . number_format($gzJson['median'], 3) . "ms -> " . formatBytes($gzJsonOut) . "\n";

$gzSconMin = benchTiming(fn() => gzencode($sconMinStr, 6), $gzIters);
$gzSconMinOut = strlen(gzencode($sconMinStr, 6));
echo "  gzip(SCON min):    " . number_format($gzSconMin['median'], 3) . "ms -> " . formatBytes($gzSconMinOut) . " (CPU " . pctChange((int)($gzJson['median']*1000), (int)($gzSconMin['median']*1000)) . ")\n";

$gzSconDM = benchTiming(fn() => gzencode($sconDedupMinStr, 6), $gzIters);
$gzSconDMOut = strlen(gzencode($sconDedupMinStr, 6));
echo "  gzip(SCON d+m):    " . number_format($gzSconDM['median'], 3) . "ms -> " . formatBytes($gzSconDMOut) . " (CPU " . pctChange((int)($gzJson['median']*1000), (int)($gzSconDM['median']*1000)) . ")\n\n";

# ============================================================
# 4. END-TO-END BY NETWORK SCENARIO
# ============================================================
echo "━━━ End-to-End Latency (encode + tx + decode) ━━━\n";

$jsonCpu = $jsonEnc['median'] + $jsonDec['median'];
$sconCpu = $sconEnc['median'] + $sconDec['median'];

$scenarios = [
    'Satellite (2 Mbps)' => 2,
    'LoRaWAN (50 kbps)' => 0.05,
    'LTE rural (10 Mbps)' => 10,
    'WiFi mine (50 Mbps)' => 50,
    'Fiber (1 Gbps)' => 1000,
];

echo str_pad('Scenario', 24) . str_pad('BW', 14)
   . str_pad('JSON (ms)', 14, ' ', STR_PAD_LEFT)
   . str_pad('SCON min (ms)', 16, ' ', STR_PAD_LEFT)
   . str_pad('Delta', 12, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 80) . "\n";

foreach ($scenarios as $name => $bwMbps) {
    $bwBytesPerMs = ($bwMbps * 1e6) / 8 / 1000;
    $jsonTx = $jsonSize / $bwBytesPerMs;
    $sconTx = $sconMinSize / $bwBytesPerMs;
    $jsonTotal = $jsonCpu + $jsonTx;
    $sconTotal = $sconCpu + $sconTx;
    $delta = $sconTotal - $jsonTotal;
    $sign = $delta < 0 ? '' : '+';

    echo str_pad($name, 24)
       . str_pad(number_format($bwMbps, $bwMbps < 1 ? 2 : 0) . ' Mbps', 14)
       . str_pad(number_format($jsonTotal, 1), 14, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sconTotal, 1), 16, ' ', STR_PAD_LEFT)
       . str_pad($sign . number_format($delta, 1), 12, ' ', STR_PAD_LEFT) . "\n";
}

echo "\n━━━ With dedup+min ━━━\n";
echo str_pad('Scenario', 24) . str_pad('BW', 14)
   . str_pad('JSON (ms)', 14, ' ', STR_PAD_LEFT)
   . str_pad('SCON d+m (ms)', 16, ' ', STR_PAD_LEFT)
   . str_pad('Delta', 12, ' ', STR_PAD_LEFT) . "\n";
echo str_repeat('─', 80) . "\n";

foreach ($scenarios as $name => $bwMbps) {
    $bwBytesPerMs = ($bwMbps * 1e6) / 8 / 1000;
    $jsonTx = $jsonSize / $bwBytesPerMs;
    $sconTx = $sconDedupMinSize / $bwBytesPerMs;
    $jsonTotal = $jsonCpu + $jsonTx;
    $sconTotal = $sconCpu + $sconTx;
    $delta = $sconTotal - $jsonTotal;
    $sign = $delta < 0 ? '' : '+';

    echo str_pad($name, 24)
       . str_pad(number_format($bwMbps, $bwMbps < 1 ? 2 : 0) . ' Mbps', 14)
       . str_pad(number_format($jsonTotal, 1), 14, ' ', STR_PAD_LEFT)
       . str_pad(number_format($sconTotal, 1), 16, ' ', STR_PAD_LEFT)
       . str_pad($sign . number_format($delta, 1), 12, ' ', STR_PAD_LEFT) . "\n";
}

# ============================================================
# 5. DAILY VOLUME PROJECTION
# ============================================================
echo "\n━━━ Daily Volume Projection (24h × 1 reading/min × 40 sensors) ━━━\n";

$readingsPerDay = 40 * 60 * 24; # 57,600 readings
$batchesPerDay = ceil($readingsPerDay / 10000); # ~6 batches
$jsonDaily = $jsonSize * $batchesPerDay;
$sconMinDaily = $sconMinSize * $batchesPerDay;
$sconDedupMinDaily = $sconDedupMinSize * $batchesPerDay;

echo "  Readings/day:      " . number_format($readingsPerDay) . " (40 sensors × 1440 min)\n";
echo "  Batches/day:       " . number_format($batchesPerDay) . " (10K readings per batch)\n";
echo "  JSON daily:        " . formatBytes($jsonDaily) . "\n";
echo "  SCON(min) daily:   " . formatBytes($sconMinDaily) . " (" . pctChange($jsonDaily, $sconMinDaily) . ")\n";
echo "  SCON(d+m) daily:   " . formatBytes($sconDedupMinDaily) . " (" . pctChange($jsonDaily, $sconDedupMinDaily) . ")\n";

$jsonMonthly = $jsonDaily * 30;
$sconDedupMinMonthly = $sconDedupMinDaily * 30;
$savedMonthly = $jsonMonthly - $sconDedupMinMonthly;
echo "\n  JSON monthly:      " . formatBytes($jsonMonthly) . "\n";
echo "  SCON(d+m) monthly: " . formatBytes($sconDedupMinMonthly) . "\n";
echo "  Saved monthly:     " . formatBytes($savedMonthly) . "\n";

echo "\nDone.\n";
