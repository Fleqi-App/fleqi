#!/usr/bin/env node
/**
 * P0-BOUNDARY-001 · check:boundaries
 * 规则（architecture.md §2、§8、§12.1）：
 *  1. crate 依赖方向：domain 不依赖 Tauri/HTTP/SQLite/系统窗口/PTY 库；application
 *     只依赖 domain；adapters 与 platform 互不依赖；两者不反向依赖 desktop。
 *  2. UI 宿主边界：packages/ui/src 中只有 adapters/host/ 可 import @tauri-apps/*；
 *     无 eval/new Function/dangerouslySetInnerHTML。
 *  3. 宿主命令白名单：apps/desktop 的 generate_handler! 命令集合 == commands.allowlist.json；
 *     capability 文件只允许这些命令；不引入 tauri-plugin-shell；CSP 不含 unsafe-eval；
 *     测试驱动（tauri-driver/webdriver）只在显式 feature 下启用。
 *  4. 契约生成：Rust 源不得使用 #[ts(export)] 自动导出。
 */
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fail, ok, repoRoot } from "./lib/contract.mjs";

let failures = 0;
console.log("check:boundaries · 模块依赖与宿主边界\n");

function walk(dir, exts, acc = []) {
  if (!existsSync(dir)) return acc;
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry === "dist" || entry === "gen") continue;
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, exts, acc);
    else if (exts.some((e) => full.endsWith(e))) acc.push(full);
  }
  return acc;
}

function cargoDeps(manifestPath) {
  const text = readFileSync(manifestPath, "utf8");
  const deps = new Set();
  for (const section of text.matchAll(/^\[(?:dev-|build-)?dependencies\]\n([\s\S]*?)(?=^\[|(?![\s\S]))/gm)) {
    for (const line of section[1].split("\n")) {
      const name = line.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
      if (name) deps.add(name);
    }
  }
  return deps;
}

// ---- 1. crate 依赖方向 ----
console.log("1. crate 依赖方向");
const INFRA_FORBIDDEN_IN_DOMAIN = [/^tauri/, /^reqwest$/, /^hyper$/, /^rusqlite$/, /^sqlx$/, /^objc2/, /^portable-pty$/, /^vt100$/, /^keyring$/, /^tokio$/];
const CRATES = {
  "fleqi-domain": { allowFleqi: [], forbid: INFRA_FORBIDDEN_IN_DOMAIN },
  "fleqi-application": { allowFleqi: ["fleqi-domain"], forbid: [/^tauri/, /^reqwest$/, /^rusqlite$/, /^objc2/, /^portable-pty$/] },
  "fleqi-adapters": { allowFleqi: ["fleqi-domain", "fleqi-application"], forbid: [/^tauri$/, /^objc2/] },
  "fleqi-platform": { allowFleqi: ["fleqi-domain", "fleqi-application"], forbid: [/^rusqlite$/, /^reqwest$/, /^portable-pty$/] },
};
for (const [crate, rule] of Object.entries(CRATES)) {
  const manifest = path.join(repoRoot, "crates", crate, "Cargo.toml");
  if (!existsSync(manifest)) {
    failures += fail(`${crate} 缺少 Cargo.toml`);
    continue;
  }
  const deps = cargoDeps(manifest);
  for (const dep of deps) {
    if (dep.startsWith("fleqi-") && !rule.allowFleqi.includes(dep)) failures += fail(`${crate} 不得依赖 ${dep}`);
    if (rule.forbid.some((re) => re.test(dep))) failures += fail(`${crate} 不得依赖基础设施库 ${dep}`);
  }
  ok(`${crate} → [${[...deps].filter((d) => d.startsWith("fleqi-")).join(", ") || "无 fleqi 依赖"}]`);
}

// ---- 2. UI 宿主边界 ----
console.log("\n2. UI 宿主边界");
const uiSrc = path.join(repoRoot, "packages/ui/src");
if (!existsSync(uiSrc)) {
  console.log("  - packages/ui 尚未建立（P0-UI-001），跳过");
} else {
  const files = walk(uiSrc, [".ts", ".tsx"]);
  const HOST_ADAPTER_DIR = path.join(uiSrc, "adapters", "host");
  for (const file of files) {
    const text = readFileSync(file, "utf8");
    const rel = path.relative(repoRoot, file);
    if (/from\s+["']@tauri-apps\//.test(text) && !file.startsWith(HOST_ADAPTER_DIR)) failures += fail(`${rel} 直接 import @tauri-apps（只允许 src/adapters/host/）`);
    if (/\beval\s*\(|new\s+Function\s*\(/.test(text)) failures += fail(`${rel} 使用 eval/new Function`);
    if (/dangerouslySetInnerHTML/.test(text)) failures += fail(`${rel} 使用 dangerouslySetInnerHTML（命令输出不得拼进 HTML）`);
  }
  ok(`${files.length} 个 UI 源文件：@tauri-apps 仅在 adapters/host/，无 eval/innerHTML`);
}

// ---- 3. 宿主命令白名单与安全配置 ----
console.log("\n3. 宿主命令白名单");
const desktopDir = path.join(repoRoot, "apps/desktop/src-tauri");
if (!existsSync(desktopDir)) {
  console.log("  - apps/desktop 尚未建立（P0-DESKTOP-001），跳过");
} else {
  const allowlistPath = path.join(repoRoot, "apps/desktop/commands.allowlist.json");
  const allowlist = existsSync(allowlistPath) ? JSON.parse(readFileSync(allowlistPath, "utf8")) : null;
  if (!allowlist) failures += fail("apps/desktop/commands.allowlist.json 缺失");
  const rustFiles = walk(path.join(desktopDir, "src"), [".rs"]);
  const registered = new Set();
  for (const file of rustFiles) {
    for (const match of readFileSync(file, "utf8").matchAll(/generate_handler!\s*\[([\s\S]*?)\]/g)) {
      for (const name of match[1].split(",").map((s) => s.trim()).filter(Boolean)) registered.add(name.split("::").at(-1));
    }
  }
  if (allowlist) {
    const allowed = new Set(allowlist.commands ?? []);
    for (const cmd of registered) if (!allowed.has(cmd)) failures += fail(`宿主注册了白名单之外的命令：${cmd}`);
    for (const cmd of allowed) if (!registered.has(cmd)) failures += fail(`白名单命令未注册：${cmd}`);
    ok(`已注册命令 == 白名单（阶段 ${allowlist.stage}）：${[...registered].join(", ") || "无"}`);

    const capFiles = walk(path.join(desktopDir, "capabilities"), [".json"]);
    for (const file of capFiles) {
      const cap = JSON.parse(readFileSync(file, "utf8"));
      for (const perm of cap.permissions ?? []) {
        const id = typeof perm === "string" ? perm : perm.identifier;
        const cmd = id?.match(/^allow-(.+)$/)?.[1]?.replace(/-/g, "_");
        if (cmd && !id.includes(":") && !allowed.has(cmd)) failures += fail(`${path.relative(repoRoot, file)} 授权了白名单之外的命令权限 ${id}`);
      }
    }
  }
  const desktopManifest = path.join(desktopDir, "Cargo.toml");
  const desktopDeps = cargoDeps(desktopManifest);
  if (desktopDeps.has("tauri-plugin-shell")) failures += fail("apps/desktop 引入了 tauri-plugin-shell（禁止裸 shell 插件）");
  const manifestText = readFileSync(desktopManifest, "utf8");
  if (/tauri-driver|webdriver/.test(manifestText) && !/\[features\][\s\S]*desktop-test/.test(manifestText)) failures += fail("测试驱动依赖未由 desktop-test feature 门控");
  const conf = JSON.parse(readFileSync(path.join(desktopDir, "tauri.conf.json"), "utf8"));
  const csp = conf.app?.security?.csp;
  if (!csp) failures += fail("tauri.conf.json 缺少 CSP");
  else if (/unsafe-eval/.test(typeof csp === "string" ? csp : JSON.stringify(csp))) failures += fail("CSP 含 unsafe-eval");
  else ok("CSP 已配置且不含 unsafe-eval");
  if (conf.identifier !== "app.fleqi.desktop") failures += fail(`bundle identifier 应为 app.fleqi.desktop：${conf.identifier}`);
}

// ---- 4. 契约生成边界 ----
console.log("\n4. 契约生成");
const rustSources = walk(path.join(repoRoot, "crates"), [".rs"]);
for (const file of rustSources) {
  if (/#\[ts\(\s*export\s*[,)]/.test(readFileSync(file, "utf8"))) failures += fail(`${path.relative(repoRoot, file)} 使用 #[ts(export)] 自动导出（普通测试会改生成文件）`);
}
ok(`${rustSources.length} 个 Rust 源文件无 #[ts(export)] 自动导出`);

console.log("");
if (failures > 0) {
  console.error(`check:boundaries 失败：${failures} 项`);
  process.exit(1);
}
console.log("check:boundaries 通过。");
