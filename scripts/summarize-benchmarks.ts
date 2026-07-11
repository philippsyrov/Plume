// D129: the summarizer reserved by docs/MODEL_BENCHMARKS.md
// § "Reserved D129 command shapes". Validates records, refuses
// unsupported schema versions, groups like-for-like attempts,
// validates pairs, and renders derived summaries. Input: sanitized
// JSONL files. Output: markdown tables on stdout, derived entirely
// from attempt records — no benchmark table is typed by hand.
//
// Run: npx --no-install vite-node scripts/summarize-benchmarks.ts -- <records.jsonl>...

import { readFileSync } from 'node:fs';

import { readRecords, renderMarkdown } from './benchmark/summarize-lib.ts';
import type { BenchmarkRecord } from './benchmark/types.ts';

const files = process.argv.slice(2);
if (files.length === 0 || files.includes('--help')) {
  process.stderr.write('usage: summarize-benchmarks.ts <records.jsonl> [more.jsonl ...]\n');
  process.exit(2);
}

const records: BenchmarkRecord[] = [];
let failed = false;
for (const file of files) {
  try {
    const result = readRecords(readFileSync(file, 'utf8'));
    result.lineErrors.forEach((e) => {
      process.stderr.write(`${file}: ${e}\n`);
      failed = true;
    });
    result.warnings.forEach((w) => process.stderr.write(`${file}: warning: ${w}\n`));
    records.push(...result.records);
  } catch (err) {
    process.stderr.write(`${file}: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
  }
}

if (records.length === 0) {
  process.stderr.write('no valid records to summarize\n');
  process.exit(1);
}
process.stdout.write(renderMarkdown(records));
process.exit(failed ? 1 : 0);
