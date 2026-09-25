#!/usr/bin/env node
// Run after the normal build and package verification; private keys stay outside Git.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { repoRoot } from "./lib/contract.mjs";

const config = JSON.parse(readFileSync(path.join(repoRoot, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
if (process.platform !== "darwin" || process.arch !== "arm64") throw new Error("当前发布目标为 Apple Silicon macOS");
const version = config.version;
const directory = path.join(repoRoot, "target/release/bundle/macos");
const archive = path.join(directory, "Fleqi.app.tar.gz");
const signature = `${archive}.sig`;
for (const file of [archive, signature]) if (!existsSync(file)) throw new Error(`缺少已签名更新包：${file}；使用 tauri.release.conf.json 构建`);
const output = path.join(repoRoot, "dist/release", version);
mkdirSync(output, { recursive: true });
const archiveName = `Fleqi_${version}_aarch64.app.tar.gz`;
const zipName = `Fleqi_${version}_aarch64.zip`;
copyFileSync(archive, path.join(output, archiveName));
copyFileSync(signature, path.join(output, `${archiveName}.sig`));
execFileSync("/usr/bin/ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", path.join(directory, "Fleqi.app"), path.join(output, zipName)]);
const notes = readFileSync(path.join(repoRoot, `docs/release-notes/${version}.md`), "utf8");
writeFileSync(path.join(output, "latest.json"), JSON.stringify({
  version, notes, pub_date: new Date().toISOString(), platforms: {
    "darwin-aarch64": {
      signature: readFileSync(signature, "utf8").trim(),
      url: `https://github.com/Fleqi-App/fleqi/releases/download/v${version}/${archiveName}`,
    },
  },
}, null, 2) + "\n");
const files = [archiveName, `${archiveName}.sig`, zipName, "latest.json"];
writeFileSync(path.join(output, "SHA256SUMS.txt"), files.map((file) => `${createHash("sha256").update(readFileSync(path.join(output, file))).digest("hex")}  ${file}\n`).join(""));
console.log(`已准备 ${version} 发布文件：${output}`);
