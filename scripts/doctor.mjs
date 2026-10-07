#!/usr/bin/env node
/**
 * P0-ENV-001：环境自检。报告版本、工具链组件、Xcode 与目标环境。
 * 关键项缺失时以非零码退出。只报告事实，不修改环境。
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statfsSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { invocation } from "./lib/process.mjs";

const failures = [];
const warnings = [];

function run(cmd, args, { allowFail = false } = {}) {
  try {
    const [command, argv] = invocation(cmd, args);
    return execFileSync(command, argv, { encoding: "utf8", timeout: 30_000, windowsHide: true }).trim();
  } catch (error) {
    if (allowFail) return null;
    throw error;
  }
}

function record(name, value, { required = true } = {}) {
  if (value == null || value === "") {
    (required ? failures : warnings).push(`${name} 不可用`);
    console.log(`✗ ${name}: 不可用`);
  } else {
    console.log(`✓ ${name}: ${value}`);
  }
}

console.log("Fleqi 开发环境自检（P0-ENV-001）\n");

console.log("— Node 与包管理 —");
record("node", process.version);
record("pnpm", run("pnpm", ["--version"], { allowFail: true }));
record("corepack", run("corepack", ["--version"], { allowFail: true }), { required: false });

const rootPkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
record("packageManager 锁定", rootPkg.packageManager);
const activePnpm = run("pnpm", ["--version"], { allowFail: true });
if (activePnpm && activePnpm !== rootPkg.packageManager.replace(/^pnpm@/, "")) {
  failures.push(`pnpm 版本 ${activePnpm} 与锁定值 ${rootPkg.packageManager} 不一致`);
}

console.log("\n— Rust 工具链 —");
const cargoBin = path.join(homedir(), ".cargo", "bin");
const exe = process.platform === "win32" ? ".exe" : "";
const rustc = existsSync(path.join(cargoBin, `rustc${exe}`)) ? path.join(cargoBin, `rustc${exe}`) : "rustc";
const rustup = existsSync(path.join(cargoBin, `rustup${exe}`)) ? path.join(cargoBin, `rustup${exe}`) : null;
record("rustup", rustup ? run(rustup, ["--version"], { allowFail: true }).split("\n")[0] : null, { required: false });
if (!rustup) {
  failures.push("rustup 未安装：rust-toolchain.toml 需要 rustup 分发（Homebrew 单体 rustc 不读取该文件）");
}
record("rustc（经 rustup shim）", run(rustc, ["--version"], { allowFail: true }));
const toolchainToml = readFileSync(new URL("../rust-toolchain.toml", import.meta.url), "utf8");
const pinned = toolchainToml.match(/channel\s*=\s*"([^"]+)"/)?.[1];
record("rust-toolchain.toml channel", pinned);
const active = rustup ? run(rustup, ["show", "active-toolchain"], { allowFail: true }) : null;
record("活动工具链", active);
if (rustup && pinned && active && !active.startsWith(pinned)) {
  failures.push(`活动工具链 ${active} 与锁定 channel ${pinned} 不一致`);
}
for (const component of ["rustfmt", "clippy"]) {
  const ok = rustup ? run(rustup, ["component", "list", "--installed"], { allowFail: true })?.split("\n").some((l) => l.startsWith(`${component}-`)) : false;
  record(`组件 ${component}`, ok ? "已安装" : null);
}

console.log("\n— 原生构建环境 —");
if (process.platform === "darwin") {
  record("xcodebuild", run("xcodebuild", ["-version"], { allowFail: true })?.split("\n")[0]);
  record("macOS SDK", run("xcrun", ["--show-sdk-version"], { allowFail: true }), { required: false });
  record("macOS", run("sw_vers", ["-productVersion"], { allowFail: true }));
} else if (process.platform === "win32") {
  if (!active?.includes("x86_64-pc-windows-msvc")) failures.push("Windows 构建要求 x86_64-pc-windows-msvc 工具链");
  const vswhere = path.join(process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)", "Microsoft Visual Studio/Installer/vswhere.exe");
  record("MSVC", run(vswhere, ["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], { allowFail: true }));
  const kits = path.join(process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)", "Windows Kits/10/Include");
  record("Windows SDK", existsSync(kits) ? kits : null);
  record("PowerShell", run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "$PSVersionTable.PSVersion.ToString()"], { allowFail: true }));
  record("WebView2 Runtime", run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "Get-ItemProperty 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\*','HKCU:\\SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\*' -ErrorAction SilentlyContinue | Where-Object { $_.name -like '*WebView2*' } | Select-Object -ExpandProperty pv"], { allowFail: true }));
} else {
  record("系统", process.platform);
}
record("架构", process.arch);
record("磁盘可用空间", (() => {
  try { const disk = statfsSync("."); return `${Math.floor(disk.bavail * disk.bsize / 1024 ** 3)} GiB`; }
  catch { return null; }
})(), { required: false });

console.log("");
if (failures.length > 0) {
  console.error(`发现 ${failures.length} 个关键问题：`);
  for (const failure of failures) console.error(`  ✗ ${failure}`);
  process.exit(1);
}
if (warnings.length > 0) {
  console.log(`提示 ${warnings.length} 项（不阻塞）：`);
  for (const warning of warnings) console.log(`  ! ${warning}`);
}
console.log("环境自检通过。");
