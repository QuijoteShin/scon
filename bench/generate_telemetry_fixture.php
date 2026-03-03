#!/usr/bin/env php
<?php
# bench/generate_telemetry_fixture.php
# Generates a realistic mining telemetry fixture:
# 10,000 sensor readings from an underground mine operation
#
# Sensor types modeled after real SCADA/IoT deployments:
# - Vibration monitors on crushers, mills, conveyors
# - Temperature probes on bearings, motors, slurry
# - Pressure transducers on hydraulic lines, pipelines
# - Flow meters on water, slurry, reagent lines
# - Level sensors on tanks, hoppers, sumps
# - Gas detectors (CH4, CO, O2, NO2) for safety
#
# All readings share the same schema (uniform objects with primitive values)
# — the exact pattern where SCON tabular encoding excels.

srand(42); # Reproducible

$fixtureDir = __DIR__ . '/fixtures';
if (!is_dir($fixtureDir)) mkdir($fixtureDir, 0755, true);

# Sensor definitions: type => [unit, min, max, locations]
$sensorTypes = [
    'vibration' => [
        'unit' => 'mm/s',
        'min' => 0.1, 'max' => 45.0,
        'locations' => ['crusher_A', 'crusher_B', 'mill_SAG', 'mill_ball', 'conveyor_01', 'conveyor_02', 'conveyor_03', 'screen_vibro'],
    ],
    'temperature' => [
        'unit' => 'C',
        'min' => 18.0, 'max' => 95.0,
        'locations' => ['bearing_motor_01', 'bearing_motor_02', 'bearing_pump_03', 'slurry_tank', 'motor_crusher_A', 'motor_mill_SAG', 'ambient_level_3', 'ambient_level_5'],
    ],
    'pressure' => [
        'unit' => 'bar',
        'min' => 0.5, 'max' => 350.0,
        'locations' => ['hydraulic_main', 'hydraulic_aux', 'pipeline_slurry', 'pipeline_water', 'pipeline_reagent', 'compressor_out'],
    ],
    'flow' => [
        'unit' => 'm3/h',
        'min' => 0.0, 'max' => 1200.0,
        'locations' => ['water_intake', 'slurry_feed', 'slurry_discharge', 'reagent_line_01', 'reagent_line_02', 'tailings_pipe'],
    ],
    'level' => [
        'unit' => '%',
        'min' => 0.0, 'max' => 100.0,
        'locations' => ['hopper_primary', 'hopper_secondary', 'sump_dewatering', 'tank_reagent', 'tank_thickener', 'silo_concentrate'],
    ],
    'gas' => [
        'unit' => 'ppm',
        'min' => 0.0, 'max' => 50.0,
        'locations' => ['CH4_level_3', 'CH4_level_5', 'CO_level_3', 'CO_level_5', 'O2_level_3', 'NO2_vent_shaft'],
    ],
];

$statuses = ['ok', 'ok', 'ok', 'ok', 'ok', 'ok', 'ok', 'ok', 'warning', 'critical'];

# Build sensor registry
$sensors = [];
$sensorId = 1;
foreach ($sensorTypes as $type => $spec) {
    foreach ($spec['locations'] as $location) {
        $sensors[] = [
            'id' => $sensorId++,
            'type' => $type,
            'location' => $location,
            'unit' => $spec['unit'],
            'min' => $spec['min'],
            'max' => $spec['max'],
        ];
    }
}

$totalSensors = count($sensors);
echo "Total sensors: $totalSensors\n";

# Generate 10,000 readings
# ~208 readings per sensor (simulating ~3.5 hours at 1 reading/min)
$readings = [];
$baseTimestamp = 1740000000; # 2025-02-19T00:00:00Z approx
$readingCount = 10000;

for ($i = 0; $i < $readingCount; $i++) {
    $sensor = $sensors[$i % $totalSensors];
    $timestamp = $baseTimestamp + ($i * 60); # 1 reading per minute
    $range = $sensor['max'] - $sensor['min'];
    $value = round($sensor['min'] + (mt_rand() / mt_getrandmax()) * $range, 2);
    $status = $statuses[array_rand($statuses)];

    $readings[] = [
        'sensor_id' => $sensor['id'],
        'ts' => $timestamp,
        'type' => $sensor['type'],
        'location' => $sensor['location'],
        'value' => $value,
        'unit' => $sensor['unit'],
        'status' => $status,
    ];
}

# Wrap in a telemetry envelope
$payload = [
    'site' => 'mine_copiapo_norte',
    'zone' => 'processing_plant',
    'batch_id' => 'batch_20250219_000000',
    'sensor_count' => $totalSensors,
    'reading_count' => $readingCount,
    'time_range' => [
        'start' => $baseTimestamp,
        'end' => $baseTimestamp + ($readingCount * 60),
    ],
    'readings' => $readings,
];

$outPath = "$fixtureDir/telemetry_mine.json";
$json = json_encode($payload, JSON_PRETTY_PRINT);
file_put_contents($outPath, json_encode($payload));

$size = strlen(json_encode($payload));
$prettySize = strlen($json);

echo "Generated: $outPath\n";
echo "Readings: " . number_format($readingCount) . "\n";
echo "JSON size: " . number_format($size) . " B (" . number_format($size / 1024, 1) . " KB)\n";
echo "JSON pretty: " . number_format($prettySize) . " B (" . number_format($prettySize / 1024, 1) . " KB)\n";
echo "Done.\n";
