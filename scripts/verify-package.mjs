#!/usr/bin/env node
/**
 * P0-PACKAGE-001 · pnpm verify:package
 * 核对 `pnpm tauri build --bundles app` 的普通 .app：Info.plist（bundle id、
 * CFBundleShortVersionString 数字形式、最低 macOS 14.0）、签名、架构、
 * 不含测试驱动，并实际启动普通包记录窗口与退出证据（tests/.artifacts/package）。
 */
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fail, ok, repoRoot } from "./lib/contract.mjs";

const APP = path.join(repoRoot, "target", "release", "bundle", "macos", "Fleqi.app");
const PLIST = path.join(APP, "Contents", "Info.plist");
const BINARY = path.join(APP, "Contents", "MacOS", "fleqi-desktop");
const ARTIFACTS = path.join(repoRoot, "tests", ".artifacts", "package");
const EXPECTED = {
  CFBundleIdentifier: "app.fleqi.desktop",
  CFBundleShortVersionString: JSON.parse(readFileSync(path.join(repoRoot, "apps/desktop/src-tauri/tauri.conf.json"), "utf8")).version,
  LSMinimumSystemVersion: "14.0",
  CFBundleName: "Fleqi",
  CFBundleExecutable: "fleqi-desktop",
};

let failures = 0;
const evidence = { app: APP, checkedAt: new Date().toISOString() };

function sh(cmd, args, { allowFail = false } = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf8" });
  if (result.status !== 0 && !allowFail) throw new Error(`${cmd} ${args.join(" ")} 失败：${result.stderr}`);
  return `${result.stdout}${result.stderr}`.trim();
}

console.log("verify:package · macOS 普通包核对\n");
if (!existsSync(APP)) {
  fail(`未找到 ${APP}，先运行 pnpm tauri build --bundles app`);
  process.exit(1);
}
mkdirSync(ARTIFACTS, { recursive: true });

console.log("1. Info.plist");
const plist = JSON.parse(sh("plutil", ["-convert", "json", "-o", "-", PLIST]));
for (const [key, expected] of Object.entries(EXPECTED)) {
  if (plist[key] !== expected) failures += fail(`${key} = ${JSON.stringify(plist[key])}，应为 ${JSON.stringify(expected)}`);
}
if (!plist.CFBundleIconFile) failures += fail("CFBundleIconFile 缺失");
if (!/^\d+(\.\d+)*$/.test(String(plist.CFBundleVersion ?? ""))) failures += fail(`CFBundleVersion 应为数字段：${plist.CFBundleVersion}`);
evidence.infoPlist = {
  CFBundleIdentifier: plist.CFBundleIdentifier,
  CFBundleShortVersionString: plist.CFBundleShortVersionString,
  CFBundleVersion: plist.CFBundleVersion,
  LSMinimumSystemVersion: plist.LSMinimumSystemVersion,
  CFBundleIconFile: plist.CFBundleIconFile,
  CFBundleExecutable: plist.CFBundleExecutable,
};
ok(`bundle ${plist.CFBundleIdentifier} · 版本 ${plist.CFBundleShortVersionString} (build ${plist.CFBundleVersion}) · 最低 macOS ${plist.LSMinimumSystemVersion}`);

console.log("\n2. 签名与架构");
const codesign = sh("codesign", ["-dv", "--verbose=2", APP], { allowFail: true });
const signature = codesign.match(/Signature=(\S+)/)?.[1] ?? codesign.match(/^Authority=(.+)$/m)?.[1] ?? "unknown";
evidence.signature = signature;
if (signature === "unknown") failures += fail(`无法读取签名信息：\n${codesign}`);
else ok(`签名：${signature}（开发/ad-hoc 签名；发行签名与公证属 M5）`);
const verify = spawnSync("codesign", ["--verify", "--deep", "--strict", APP], { encoding: "utf8" });
if (verify.status !== 0) failures += fail(`codesign --verify 失败：${verify.stderr}`);
else ok("codesign --verify --deep --strict 通过");
const archs = sh("lipo", ["-archs", BINARY]);
evidence.architectures = archs.split(/\s+/);
ok(`架构：${archs}`);
if (!archs.split(/\s+/).includes(process.arch === "arm64" ? "arm64" : "x86_64")) failures += fail("二进制不含当前机器架构");

console.log("\n3. 不含测试驱动");
const binary = readFileSync(BINARY);
for (const marker of ["TAURI_WEBDRIVER_PORT", "wdio_webdriver", "tauri-plugin-wdio-webdriver"]) {
  if (binary.includes(marker)) failures += fail(`普通包二进制含测试驱动标记 ${marker}`);
}
ok("二进制不含 WebDriver/wdio 标记");
evidence.testDriverMarkers = "absent";

console.log("\n4. 启动普通包");
const before = sh("pgrep", ["-x", "fleqi-desktop"], { allowFail: true });
if (before) failures += fail(`已有 fleqi-desktop 进程：${before}`);
sh("open", ["-n", APP]);
const launchedAt = new Date().toISOString();
let pid = "";
const deadline = Date.now() + 20_000;
while (!pid && Date.now() < deadline) {
  execFileSync("sleep", ["0.25"]);
  pid = sh("pgrep", ["-x", "fleqi-desktop"], { allowFail: true }).split("\n")[0];
}
if (!pid) {
  failures += fail("普通包未在 20s 内启动");
} else {
  execFileSync("sleep", ["2.5"]);
  let windows = [];
  try {
    windows = JSON.parse(sh("swift", [path.join(repoRoot, "scripts", "lib", "window-info.swift"), pid]));
  } catch (error) {
    failures += fail(`读取窗口信息失败：${error.message}`);
  }
  const main = windows.find((w) => w.layer === 0 && w.bounds?.Width >= 640);
  if (!main) failures += fail(`未观察到宿主窗口（pid ${pid}）：${JSON.stringify(windows)}`);
  else ok(`窗口 "${main.title}" ${main.bounds.Width}×${main.bounds.Height} @ (${main.bounds.X}, ${main.bounds.Y})`);
  let screenshot = null;
  if (main) {
    screenshot = path.join(ARTIFACTS, "fleqi-app-window.png");
    const cap = spawnSync("screencapture", ["-x", "-o", `-l${main.windowId}`, screenshot], { encoding: "utf8" });
    if (cap.status !== 0 || !existsSync(screenshot)) {
      console.log(`  ! 窗口截图不可用（需要屏幕录制权限）：${cap.stderr.trim()}`);
      screenshot = null;
    } else ok(`窗口截图：${screenshot}`);
  }
  sh("osascript", ["-e", 'tell application id "app.fleqi.desktop" to quit'], { allowFail: true });
  const exitDeadline = Date.now() + 15_000;
  let remaining = pid;
  while (remaining && Date.now() < exitDeadline) {
    execFileSync("sleep", ["0.25"]);
    remaining = sh("pgrep", ["-x", "fleqi-desktop"], { allowFail: true });
  }
  if (remaining) {
    failures += fail(`退出后仍有进程：${remaining}`);
    spawnSync("kill", [remaining]);
  } else ok("quit 后进程已退出，无残留");
  evidence.launch = { pid: Number(pid), launchedAt, windows, screenshot, quitAt: new Date().toISOString(), exitClean: !remaining };
}

evidence.macos = sh("sw_vers", ["-productVersion"]);
evidence.hardware = sh("uname", ["-m"]);
evidence.passed = failures === 0;
writeFileSync(path.join(ARTIFACTS, "package-evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
console.log(`\n证据：${path.join(ARTIFACTS, "package-evidence.json")}`);
if (failures > 0) {
  console.error(`verify:package 失败：${failures} 项`);
  process.exit(1);
}
console.log("verify:package 通过。");
