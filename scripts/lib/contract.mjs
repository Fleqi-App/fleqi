/**
 * 合同检查共享库：文档集合、ID 家族、Markdown 链接/锚点与格式规则。
 */
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
export const docsDir = path.join(repoRoot, "docs");

/** 六份合同 + 状态/回退记录。docs/superpowers/ 是实施计划，不入合同扫描。 */
export const CONTRACT_DOCS = [
  "README.md",
  "requirements.md",
  "capabilities.md",
  "ui-design.md",
  "architecture.md",
  "development-plan.md",
  "status.md",
  "restart.md",
];

export function readDoc(name) {
  return readFileSync(path.join(docsDir, name), "utf8");
}

/** ID 家族 → 定义文档。引用处扫描全部合同文档。 */
export const ID_FAMILIES = [
  { re: /\b(AC-CAP-\d{3})\b/g, definer: "capabilities.md" },
  { re: /\b(CAP-[A-Z]+-\d{3})\b/g, definer: "capabilities.md" },
  { re: /\b(AC-COMMON-\d{3})\b/g, definer: "capabilities.md" },
  { re: /\b(DEP-[A-Z]+(?:-[A-Z]+)*)\b/g, definer: "capabilities.md" },
  { re: /\b(AC-FLOW-\d{3})\b/g, definer: "requirements.md" },
  { re: /\b(FR-[A-Z]+-\d{3})\b/g, definer: "requirements.md" },
  { re: /\b(NFR-[A-Z]+-\d{3})\b/g, definer: "requirements.md" },
  { re: /\b(UI-[A-Z][A-Z0-9-]*[A-Z0-9])\b/g, definer: "ui-design.md" },
  { re: /\b(UX-[A-Z]+-\d{2})\b/g, definer: "ui-design.md" },
  { re: /\b(ADR-\d{3})\b/g, definer: "architecture.md" },
];

/** GitHub 风格标题 slug（CJK 保留，标点删除，空格转连字符）。 */
export function headingSlug(text) {
  return text
    .toLowerCase()
    .replace(/\p{P}/gu, "")
    .replace(/\p{Z}/gu, "-");
}

export function collectAnchors(markdown) {
  const anchors = new Set();
  for (const match of markdown.matchAll(/^#{1,6}[ \t]+(.+?)[ \t]*#*[ \t]*$/gm)) {
    anchors.add(headingSlug(match[1].trim()));
  }
  return anchors;
}

/** 提取 Markdown 内链（跳过外链 http/https、mailto）。返回 [{ from, target, anchor, raw }] */
export function collectLinks(fromDoc, markdown) {
  const links = [];
  for (const match of markdown.matchAll(/\[[^\]]*\]\(([^)\s]+)(?:[ \t]+"[^"]*")?\)/g)) {
    const raw = match[1];
    if (/^(https?:|mailto:|#)/i.test(raw)) continue;
    const [target, anchor] = decodeURIComponent(raw).split("#");
    links.push({ from: fromDoc, target, anchor, raw });
  }
  return links;
}

/** editorconfig 合同：UTF-8 可解码、LF、末尾换行。 */
export function checkTextFormat(name, buffer) {
  const issues = [];
  const text = buffer.toString("utf8");
  const replacement = buffer.indexOf(Buffer.from([0xef, 0xbf, 0xbd]));
  if (replacement !== -1) issues.push("UTF-8 解码出现替换字符");
  if (text.includes("\r")) issues.push("包含 CR（要求 LF）");
  if (!text.endsWith("\n")) issues.push("缺少末尾换行");
  if (text.endsWith("\n\n")) issues.push("末尾多余空行");
  return issues;
}

export function readDocBuffer(name) {
  const file = path.join(docsDir, name);
  if (!existsSync(file)) return null;
  return readFileSync(file);
}

export function fail(message) {
  console.error(`  ✗ ${message}`);
  return 1;
}

export function ok(message) {
  console.log(`  ✓ ${message}`);
}
