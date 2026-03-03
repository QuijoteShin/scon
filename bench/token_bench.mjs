// bench/token_bench.mjs — Token count comparison JSON vs SCON (cl100k_base)
import { readFileSync, writeFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { encode } from 'gpt-tokenizer';

const __dirname = dirname(fileURLToPath(import.meta.url));
const baseDir = join(__dirname, '..');
const sconDir = join(baseDir, 'js');
const { default: SCON } = await import(join(sconDir, 'scon.js'));

const fixtures = {
  'OpenAPI Specs': join(baseDir, 'bench/fixtures/openapi_specs.json'),
  'Config Records': join(baseDir, 'bench/fixtures/config_records.json'),
  'DB Exports': join(baseDir, 'bench/fixtures/db_exports.json'),
};

console.log('Token count: JSON vs SCON (cl100k_base / GPT-4 tokenizer)\n');

const header = ['Dataset', 'JSON_min', 'JSON_pretty', 'SCON', 'SCON_min', 'Δ tok%', 'B/tok JSON', 'B/tok SCON'];
console.log(header.map((h,i) => i === 0 ? h.padEnd(18) : h.padStart(12)).join(''));
console.log('-'.repeat(102));

const results = [];

for (const [name, path] of Object.entries(fixtures)) {
  const raw = readFileSync(path, 'utf8');
  const data = JSON.parse(raw);

  const jsonMin = JSON.stringify(data);
  const jsonPretty = JSON.stringify(data, null, 2);
  const scon = SCON.encode(data);
  const sconMin = SCON.minify(scon);

  const tokJsonMin = encode(jsonMin).length;
  const tokJsonPretty = encode(jsonPretty).length;
  const tokScon = encode(scon).length;
  const tokSconMin = encode(sconMin).length;

  const deltaPct = ((tokSconMin - tokJsonMin) / tokJsonMin * 100).toFixed(1);
  const bptJson = (Buffer.byteLength(jsonMin) / tokJsonMin).toFixed(2);
  const bptScon = (Buffer.byteLength(sconMin) / tokSconMin).toFixed(2);

  console.log(
    name.padEnd(18),
    String(tokJsonMin).padStart(12),
    String(tokJsonPretty).padStart(12),
    String(tokScon).padStart(12),
    String(tokSconMin).padStart(12),
    (deltaPct + '%').padStart(12),
    bptJson.padStart(12),
    bptScon.padStart(12)
  );

  results.push({
    dataset: name,
    tokens: { json_min: tokJsonMin, json_pretty: tokJsonPretty, scon: tokScon, scon_min: tokSconMin },
    delta_pct: parseFloat(deltaPct),
    bytes_per_token: { json_min: parseFloat(bptJson), scon_min: parseFloat(bptScon) },
    bytes: { json_min: Buffer.byteLength(jsonMin), scon_min: Buffer.byteLength(sconMin) }
  });
}

// Save results
const outPath = join(baseDir, 'bench/datasets/tokens_cl100k.json');
writeFileSync(outPath, JSON.stringify({ tokenizer: 'cl100k_base', model_family: 'GPT-4/Claude', results }, null, 2));
console.log('\nSaved to:', outPath);
