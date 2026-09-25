#!/usr/bin/env node
// Validate recorded evidence only. Run the real Rust matrix before this check.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const input = process.argv[2]
  ? path.resolve(process.argv[2])
  : path.join(root, 'tests/.artifacts/ac-matrix/extended.json');
const records = JSON.parse(await readFile(input, 'utf8'));
const expected = Array.from({ length: 105 }, (_, index) =>
  `AC-CAP-${String(index + 31).padStart(3, '0')}`,
);
const counts = { pass: 0, cond: 0, gap: 0, fail: 0 };
if (!Array.isArray(records) || records.length !== expected.length) {
  throw new Error('扩展矩阵必须记录全部 105 项；局部测试产物不能作为完整证据');
}
for (const [index, row] of records.entries()) {
  if (row.ac !== expected[index]) {
    throw new Error(`编号必须唯一、连续且排序固定：预期 ${expected[index]}，实际 ${row.ac}`);
  }
  if (!Object.hasOwn(counts, row.verdict)) {
    throw new Error(`${row.ac} 的 verdict 无效`);
  }
  if (typeof row.note !== 'string' || !row.note.trim()) {
    throw new Error(`${row.ac} 缺少真实验证范围/条件说明`);
  }
  counts[row.verdict] += 1;
}
console.log(`扩展矩阵证据：105 项；pass=${counts.pass}，cond=${counts.cond}，gap=${counts.gap}，fail=${counts.fail}`);
console.log('cond 仅表示已记录条件与局部证据，不代表该项完整验收通过。');
if (counts.fail || counts.gap) process.exitCode = 1;
