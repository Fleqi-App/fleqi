import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { release } from "node:os";
import { fileURLToPath } from "node:url";

export const here = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(here, "../../..");
export const artifactsDir = path.join(repoRoot, "tests/.artifacts/desktop");

/** 测试构建二进制：固定为 workspace target/debug 产物（scripts/test-desktop.mjs 负责构建）。 */
export const testBinary = path.join(repoRoot, "target", "debug", process.platform === "win32" ? "fleqi-desktop.exe" : "fleqi-desktop");

export function ensureArtifactsDir(): string {
  mkdirSync(artifactsDir, { recursive: true });
  return artifactsDir;
}

const EVIDENCE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,80}\.(json|png|txt)$/;

/** 只接受白名单形式的纯文件名，目标必须落在 artifactsDir 内。 */
export function evidencePath(name: string): string {
  if (!EVIDENCE_NAME.test(name) || name.includes("..")) {
    throw new Error(`非法证据文件名：${name}`);
  }
  const target = path.resolve(artifactsDir, name);
  if (!target.startsWith(artifactsDir + path.sep)) {
    throw new Error(`证据路径越出目录：${name}`);
  }
  return target;
}

export function writeEvidence(name: string, data: unknown): string {
  ensureArtifactsDir();
  const file = evidencePath(name);
  writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`);
  return file;
}

/** 当前仍在运行的测试构建进程 PID 列表（空数组表示已全部退出）。 */
export function runningTestBinaryPids(): number[] {
  try {
    const out = process.platform === "win32"
      ? execFileSync("powershell.exe", ["-NoProfile", "-Command", "Get-CimInstance Win32_Process -Filter \"name = 'fleqi-desktop.exe'\" | Where-Object { $_.ExecutablePath -eq $env:FLEQI_TEST_BINARY } | Select-Object -ExpandProperty ProcessId"], { env: { ...process.env, FLEQI_TEST_BINARY: testBinary }, encoding: "utf8", windowsHide: true })
      : execFileSync("pgrep", ["-f", "--", testBinary], { encoding: "utf8" });
    return out
      .split("\n")
      .filter(Boolean)
      .map(Number)
      .filter((pid) => Number.isInteger(pid) && pid !== process.pid);
  } catch {
    return [];
  }
}

export function hostFacts() {
  if (process.platform === "win32") return { platform: process.platform, nodeArch: process.arch, windowsVersion: release() };
  const sw = (flag: "-productVersion" | "-buildVersion") => execFileSync("sw_vers", [flag], { encoding: "utf8" }).trim();
  return {
    platform: process.platform,
    nodeArch: process.arch,
    macosVersion: sw("-productVersion"),
    macosBuild: sw("-buildVersion"),
    hardware: execFileSync("uname", ["-m"], { encoding: "utf8" }).trim(),
  };
}
