#!/usr/bin/env node
/**
 * P0-DOC-001 · check:docs
 * 输入为当前合同文档集（docs/ 六份合同 + 状态/回退记录）。
 * 检查：文件存在与格式（UTF-8/LF/末尾换行）、内链与锚点可达、跨文档 ID 引用
 * 有定义、三组枚举全文同一取值、许可证/第三方/构建/贡献说明就位。
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import {
  CONTRACT_DOCS,
  ID_FAMILIES,
  checkTextFormat,
  collectAnchors,
  collectLinks,
  docsDir,
  fail,
  ok,
  readDocBuffer,
  repoRoot,
} from "./lib/contract.mjs";

let failures = 0;
const docs = new Map();

console.log("check:docs · 合同文档一致性\n");

console.log("1. 文件与格式");
for (const name of CONTRACT_DOCS) {
  const buffer = readDocBuffer(name);
  if (!buffer) {
    failures += fail(`${name} 不存在`);
    continue;
  }
  const issues = checkTextFormat(name, buffer);
  for (const issue of issues) failures += fail(`${name}: ${issue}`);
  docs.set(name, buffer.toString("utf8"));
}
if (docs.size === CONTRACT_DOCS.length) ok(`${docs.size} 份文档存在且格式合规`);

console.log("\n2. 内链与锚点");
let linkCount = 0;
for (const [name, text] of docs) {
  for (const link of collectLinks(name, text)) {
    linkCount += 1;
    const targetPath = link.target ? path.resolve(docsDir, link.target) : path.join(docsDir, name);
    if (!targetPath.startsWith(repoRoot)) {
      failures += fail(`${name}: 链接 ${link.raw} 指向仓库之外`);
      continue;
    }
    if (!existsSync(targetPath)) {
      failures += fail(`${name}: 链接目标不存在 ${link.raw}`);
      continue;
    }
    if (link.anchor && targetPath.endsWith(".md")) {
      const anchors = collectAnchors(readFileSync(targetPath, "utf8"));
      if (!anchors.has(link.anchor)) {
        failures += fail(`${name}: 锚点不存在 ${link.raw}（可用：${[...anchors].filter((a) => a.includes(link.anchor.slice(0, 4))).join(", ") || "无近似"}）`);
      }
    }
  }
}
ok(`${linkCount} 个内链已解析`);

console.log("\n3. 跨文档 ID 引用");
const defined = new Map();
for (const family of ID_FAMILIES) {
  const text = docs.get(family.definer) ?? "";
  const set = defined.get(family.definer) ?? new Set();
  for (const match of text.matchAll(family.re)) set.add(match[1]);
  defined.set(family.definer, set);
}
let citations = 0;
const missing = [];
for (const [name, text] of docs) {
  for (const family of ID_FAMILIES) {
    for (const match of text.matchAll(family.re)) {
      citations += 1;
      if (!defined.get(family.definer).has(match[1])) missing.push(`${name} 引用 ${match[1]}，但 ${family.definer} 未定义`);
    }
  }
}
for (const item of [...new Set(missing)]) failures += fail(item);
ok(`${citations} 处 ID 引用已核对`);

console.log("\n4. 枚举取值与默认值一致（需求 §2.1 为唯一来源）");
const ENUMS = [
  { field: "activation", allowed: ["manual", "followFinder"] },
  { field: "hideBehavior", allowed: ["keepAll", "endAll"] },
  { field: "aiPolicy", allowed: ["yolo", "readOnlyAutoConfirmChanges"] },
];
const requirements = docs.get("requirements.md") ?? "";
const uiDesign = docs.get("ui-design.md") ?? "";
const architecture = docs.get("architecture.md") ?? "";
const defaultsSection = requirements.slice(requirements.indexOf("### 2.1"), requirements.indexOf("\n## 3."));
if (!defaultsSection.startsWith("### 2.1")) failures += fail("requirements.md 缺少 §2.1 设计默认值小节");

function requirementsDefaultRow(field) {
  const row = defaultsSection.match(new RegExp(`^\\| ${field} \\| ([^|]+) \\|`, "m"));
  return row?.[1].trim() ?? null;
}
function uiDefaultCell(field) {
  const row = uiDesign.match(new RegExp("^\\| [^|]+ \\| `" + field + "` \\| `?([^`|]+?)`? \\|", "m"));
  return row?.[1].trim() ?? null;
}

for (const spec of ENUMS) {
  const row = requirementsDefaultRow(spec.field);
  if (!row) {
    failures += fail(`requirements.md §2.1 缺少 ${spec.field} 行`);
    continue;
  }
  const tokens = [...new Set(row.match(/\b[a-z][A-Za-z]+\b/g) ?? [])];
  const extra = tokens.filter((t) => !spec.allowed.includes(t));
  const absent = spec.allowed.filter((t) => !tokens.includes(t));
  if (extra.length || absent.length) failures += fail(`${spec.field} 需求默认值行取值集合 {${tokens.join(", ")}} ≠ {${spec.allowed.join(", ")}}`);
  for (const value of spec.allowed) {
    for (const [docName, text] of [["ui-design.md", uiDesign], ["architecture.md", architecture]]) {
      if (!text.includes(value)) failures += fail(`${docName} 未出现 ${spec.field} 取值 ${value}`);
    }
  }
  ok(`${spec.field} ∈ {${spec.allowed.join(", ")}}`);
}

const SHARED_DEFAULTS = [
  "barEnabled", "activation", "hotkey", "hideBehavior", "aiPolicy", "launchAtLogin", "theme",
  "bubbleSeconds", "inlineSuggestionsEnabled", "inlineSuggestionsLimit", "transparency", "motionMode", "terminalFontSize",
];
let defaultsChecked = 0;
for (const field of SHARED_DEFAULTS) {
  const reqRow = requirementsDefaultRow(field);
  const uiCell = uiDefaultCell(field);
  if (!reqRow || !uiCell) {
    failures += fail(`${field}：需求(${reqRow ? "有" : "无"}) / UI(${uiCell ? "有" : "无"}) 默认值行缺失`);
    continue;
  }
  const reqDefault = reqRow.split(/[；。，]/)[0].trim();
  if (reqDefault !== uiCell) failures += fail(`${field} 默认值不一致：需求 "${reqDefault}" vs UI "${uiCell}"`);
  defaultsChecked += 1;
}
ok(`${defaultsChecked} 个共享字段默认值需求/UI 一致`);
const forbiddenPolicyPhrases = [/第三种[^。]*策略[^。]*(?:允许|支持|可以)/];
for (const [name, text] of docs) {
  for (const re of forbiddenPolicyPhrases) {
    if (re.test(text)) failures += fail(`${name}: 出现与"仅两种 AI 策略"冲突的表述`);
  }
}

console.log("\n5. 项目说明文件");
const rootFiles = [
  ["LICENSE", /GNU AFFERO GENERAL PUBLIC LICENSE/],
  ["THIRD_PARTY_NOTICES.md", /./],
  ["CONTRIBUTING.md", /./],
  ["README.md", /AGPL/],
  ["AGENTS.md", /docs\/README\.md/],
];
for (const [file, probe] of rootFiles) {
  const full = path.join(repoRoot, file);
  if (!existsSync(full)) {
    failures += fail(`${file} 缺失`);
  } else if (!probe.test(readFileSync(full, "utf8"))) {
    failures += fail(`${file} 内容不符合预期（${probe}）`);
  } else {
    ok(file);
  }
}

console.log("");
if (failures > 0) {
  console.error(`check:docs 失败：${failures} 项`);
  process.exit(1);
}
console.log("check:docs 通过。");
