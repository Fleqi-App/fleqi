#!/usr/bin/env node
/**
 * P0-REPO-001 · check:references
 * 校验 docs/reference-assets.json：逐项核对路径存在、字节数与 SHA-256；
 * storage=git 的项必须被 Git 跟踪；Web APP/ 与 Icon/ 根下出现未登记的被跟踪
 * 文件视为漂移（失败）。localOnly 项（本机视频）缺失时告警不失败（新克隆无该
 * 文件不阻塞构建），存在时仍须哈希一致。
 */
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fail, ok, repoRoot } from "./lib/contract.mjs";

async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

function git(args, { allowFail = false } = {}) {
  try {
    return execFileSync("git", ["-c", "core.quotepath=false", ...args], { cwd: repoRoot, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  } catch (error) {
    if (allowFail) return "";
    throw error;
  }
}

let failures = 0;
const warnings = [];

console.log("check:references · 参考资产清单\n");

const manifest = JSON.parse(await readFile(path.join(repoRoot, "docs/reference-assets.json"), "utf8"));
if (manifest.schemaVersion !== 1) failures += fail(`schemaVersion 不支持：${manifest.schemaVersion}`);
const assets = manifest.assets ?? [];
console.log(`清单：${assets.length} 项（capturedAt ${manifest.capturedAt}）\n`);

let verified = 0;
for (const asset of assets) {
  const file = path.join(repoRoot, asset.path);
  const label = `${asset.path} [${asset.storage}]`;
  if (!asset.sha256 || typeof asset.bytes !== "number") {
    failures += fail(`${label} 清单条目缺少 bytes/sha256`);
    continue;
  }
  try {
    const info = await stat(file);
    if (info.size !== asset.bytes) {
      failures += fail(`${label} 字节数 ${info.size} ≠ ${asset.bytes}`);
      continue;
    }
    const digest = await sha256(file);
    if (digest !== asset.sha256) {
      failures += fail(`${label} SHA-256 不一致`);
      continue;
    }
  } catch {
    if (asset.storage === "localOnly") {
      warnings.push(`${label} 本机不存在（新克隆环境正常，视频仅本机保留）`);
      continue;
    }
    failures += fail(`${label} 文件不存在`);
    continue;
  }
  if (asset.storage === "git") {
    const tracked = git(["ls-files", "--", asset.path]).trim();
    if (tracked !== asset.path) failures += fail(`${label} 未被 Git 跟踪`);
    const ignored = git(["check-ignore", asset.path], { allowFail: true }).trim();
    if (ignored) failures += fail(`${label} 被忽略规则命中：${ignored}`);
  }
  verified += 1;
}
ok(`${verified}/${assets.length} 项路径、字节与 SHA-256 一致`);

const listed = new Set(assets.map((a) => a.path));
const trackedFiles = git(["ls-files"]).split("\n").filter(Boolean);
const drift = trackedFiles.filter((p) => (p.startsWith("Web APP/") || p.startsWith("Icon/")) && !listed.has(p));
if (drift.length > 0) {
  for (const file of drift) failures += fail(`漂移：${file} 被 Git 跟踪但未登记到清单`);
} else {
  ok("Web APP/ 与 Icon/ 根下无未登记的被跟踪文件");
}

console.log("");
for (const warning of warnings) console.log(`  ! ${warning}`);
if (failures > 0) {
  console.error(`check:references 失败：${failures} 项`);
  process.exit(1);
}
console.log("check:references 通过。");
