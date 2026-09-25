#!/usr/bin/env node
/**
 * P0-DOC-001 · check:traceability
 * 解析能力台账与需求/计划，核验：
 *  - AC-CAP-001..135 连续唯一；30 个基础 CAP 各有独立 AC（001..030）；
 *  - 105 个 legacy_id 唯一，十类计数 10/18/7/12/5/20/4/13/12/4，各行映射到已定义 CAP；
 *  - AC-FLOW-001..015 在需求中定义且被开发计划全部引用；
 *  - AC-CAP 全部被开发计划的阶段范围覆盖；FR/NFR ID 唯一，需求族在追踪表出现。
 */
import { readDoc, fail, ok } from "./lib/contract.mjs";

let failures = 0;
const capabilities = readDoc("capabilities.md");
const requirements = readDoc("requirements.md");
const plan = readDoc("development-plan.md");

console.log("check:traceability · 需求/能力/验收追踪\n");

function section(text, startMarker, endMarker) {
  const start = text.indexOf(startMarker);
  const end = endMarker ? text.indexOf(endMarker, start + 1) : text.length;
  if (start === -1 || end === -1) throw new Error(`找不到小节 ${startMarker} … ${endMarker}`);
  return text.slice(start, end);
}

function tableRows(text) {
  return text
    .split("\n")
    .filter((line) => line.startsWith("| ") && !line.startsWith("|---") && !/^\| (CAP ID|legacy_id|公共 ID|依赖 ID|范围|核对项) /.test(line))
    .map((line) => line.slice(1, -1).split(" | ").map((cell) => cell.trim()));
}

function expandRanges(text, family) {
  const found = new Set();
  const re = new RegExp(`${family}-(\\d{3}(?:[–-]\\d{3})?(?:、\\d{3}(?:[–-]\\d{3})?)*)`, "g");
  for (const match of text.matchAll(re)) {
    for (const segment of match[1].split("、")) {
      const [from, to] = segment.split(/[–-]/).map(Number);
      for (let i = from; i <= (to ?? from); i += 1) found.add(`${family}-${String(i).padStart(3, "0")}`);
    }
  }
  return found;
}

// ---- 1. 基础能力 30 项 ----
console.log("1. 基础能力");
const baseRows = tableRows(section(capabilities, "## 2. 六类基础能力", "## 3. 历史操作意图")).filter((r) => r[0].startsWith("CAP-"));
const baseCaps = baseRows.map((r) => r[0]);
const baseAcs = baseRows.map((r) => r.at(-1).match(/^(AC-CAP-\d{3})：/)?.[1] ?? null);
if (baseRows.length !== 30) failures += fail(`基础能力应为 30 行，实际 ${baseRows.length}`);
if (new Set(baseCaps).size !== baseCaps.length) failures += fail("基础 CAP ID 重复");
const baseFamilies = new Set(baseCaps.map((c) => c.split("-")[1]));
if (baseFamilies.size !== 6) failures += fail(`基础能力应为六类，实际 ${[...baseFamilies].join(",")}`);
baseAcs.forEach((ac, i) => {
  const expected = `AC-CAP-${String(i + 1).padStart(3, "0")}`;
  if (ac !== expected) failures += fail(`${baseCaps[i]} 的验收应为 ${expected}，实际 ${ac}`);
});
ok(`${baseRows.length} 项基础能力，${baseFamilies.size} 类，AC-CAP-001..030 一一对应`);

// ---- 2. 历史意图 105 项 ----
console.log("\n2. 历史操作意图");
const legacySection = section(capabilities, "## 3. 历史操作意图", "## 4. 映射边界");
const EXPECTED_COUNTS = [10, 18, 7, 12, 5, 20, 4, 13, 12, 4];
const categories = legacySection.split(/^### /m).slice(1);
if (categories.length !== 10) failures += fail(`历史类目应为 10 个，实际 ${categories.length}`);
const legacyRows = [];
categories.forEach((chunk, index) => {
  const heading = chunk.split("\n")[0];
  const declared = Number(heading.match(/：(\d+) 项/)?.[1]);
  const rows = tableRows(chunk).filter((r) => /^[a-z0-9]+(\.[a-z0-9-]+)+$/.test(r[0]));
  if (rows.length !== EXPECTED_COUNTS[index]) failures += fail(`类目 ${heading} 应 ${EXPECTED_COUNTS[index]} 行，实际 ${rows.length}`);
  if (declared !== rows.length) failures += fail(`类目 ${heading} 标题声明 ${declared} 项与实际 ${rows.length} 不符`);
  legacyRows.push(...rows);
});
const legacyIds = legacyRows.map((r) => r[0]);
if (legacyIds.length !== 105) failures += fail(`legacy_id 应为 105 个，实际 ${legacyIds.length}`);
if (new Set(legacyIds).size !== legacyIds.length) failures += fail("legacy_id 存在重复");
const allCaps = new Set(baseCaps);
for (const row of legacyRows) {
  if (!/^CAP-[A-Z]+-\d{3}$/.test(row[1])) failures += fail(`${row[0]} 映射的能力 ID 非法：${row[1]}`);
  allCaps.add(row[1]);
  if (row.length !== 7) failures += fail(`${row[0]} 列数应为 7（CAP、参数、输出、依赖、平台/条件、验收），实际 ${row.length}`);
  if (!/^DEP-/.test(row[4])) failures += fail(`${row[0]} 缺少依赖列`);
}
ok(`${legacyIds.length} 个唯一 legacy_id，十类计数 ${EXPECTED_COUNTS.join("/")}，映射 ${allCaps.size} 个能力 ID`);

// ---- 3. 验收 ID 连续唯一 ----
console.log("\n3. 能力验收 ID");
const legacyAcs = legacyRows.map((r) => r.at(-1).match(/^(AC-CAP-\d{3})：/)?.[1] ?? null);
const allAcs = [...baseAcs, ...legacyAcs];
if (allAcs.some((a) => a == null)) failures += fail("存在没有 AC-CAP 前缀的验收单元格");
if (new Set(allAcs).size !== allAcs.length) failures += fail("AC-CAP 存在重复");
for (let i = 1; i <= 135; i += 1) {
  const id = `AC-CAP-${String(i).padStart(3, "0")}`;
  if (!allAcs.includes(id)) failures += fail(`缺少 ${id}`);
}
if (allAcs.length !== 135) failures += fail(`AC-CAP 应为 135 项，实际 ${allAcs.length}`);
const planAcCaps = expandRanges(plan, "AC-CAP");
const uncovered = allAcs.filter((a) => !planAcCaps.has(a));
if (uncovered.length) failures += fail(`开发计划未覆盖：${uncovered.slice(0, 5).join(", ")}${uncovered.length > 5 ? "…" : ""}`);
ok(`AC-CAP-001..135 连续唯一，开发计划覆盖 ${planAcCaps.size} 项`);

// ---- 4. 联合验收 FLOW ----
console.log("\n4. 联合验收");
const flowDefined = [...requirements.matchAll(/^\| (AC-FLOW-\d{3}) \|/gm)].map((m) => m[1]);
if (new Set(flowDefined).size !== flowDefined.length) failures += fail("AC-FLOW 定义重复");
for (let i = 1; i <= 15; i += 1) {
  const id = `AC-FLOW-${String(i).padStart(3, "0")}`;
  if (!flowDefined.includes(id)) failures += fail(`需求缺少 ${id}`);
}
if (flowDefined.length !== 15) failures += fail(`AC-FLOW 应为 15 项，实际 ${flowDefined.length}`);
const planFlows = expandRanges(plan, "AC-FLOW");
const flowUncovered = flowDefined.filter((f) => !planFlows.has(f));
if (flowUncovered.length) failures += fail(`开发计划未引用：${flowUncovered.join(", ")}`);
ok(`AC-FLOW-001..015 定义完整，开发计划引用 ${planFlows.size} 项`);

// ---- 5. 需求 ID ----
console.log("\n5. 功能/非功能需求");
const frIds = [...requirements.matchAll(/^\| ((?:N?FR)-[A-Z]+-\d{3}) \|/gm)].map((m) => m[1]);
if (new Set(frIds).size !== frIds.length) {
  const dup = frIds.filter((id, i) => frIds.indexOf(id) !== i);
  failures += fail(`需求 ID 重复：${[...new Set(dup)].join(", ")}`);
}
const families = new Set(frIds.map((id) => id.replace(/-\d{3}$/, "")));
const tracking = section(plan, "## 3. 需求到实现", "## 4.");
const trackingFamilies = new Set();
for (const match of tracking.matchAll(/\b(N?FR)-([A-Z]+(?:\/[A-Z]+)*)/g)) {
  for (const suffix of match[2].split("/")) trackingFamilies.add(`${match[1]}-${suffix}`);
}
for (const family of families) {
  if (!trackingFamilies.has(family)) failures += fail(`开发计划 §3 追踪表未出现需求族 ${family}`);
}
const flowRefs = [...requirements.matchAll(/^\| AC-FLOW-\d{3} \| [^|]+ \| ([^|]+) \|/gm)].flatMap((m) => m[1].match(/(?:N?FR)-[A-Z]+-\d{3}(?:\/\d{3})*/g) ?? []);
for (const ref of flowRefs) {
  const [head, ...rest] = ref.split("/");
  const base = head.replace(/-\d{3}$/, "");
  for (const num of [head.match(/\d{3}$/)[0], ...rest]) {
    const id = `${base}-${num}`;
    if (!frIds.includes(id)) failures += fail(`AC-FLOW 关联需求 ${id} 未定义`);
  }
}
ok(`${frIds.length} 个 FR/NFR ID 唯一，${families.size} 个需求族均在追踪表；FLOW 关联需求全部有定义`);

console.log("");
if (failures > 0) {
  console.error(`check:traceability 失败：${failures} 项`);
  process.exit(1);
}
console.log("check:traceability 通过：135 AC-CAP、105 legacy_id、15 AC-FLOW 全覆盖。");
