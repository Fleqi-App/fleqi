import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import crypto from "node:crypto";
import net from "node:net";
import path from "node:path";
import { repoRoot } from "./lib/contract.mjs";

const version = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8")).version;
const binary = path.join(repoRoot, "target/release/fleqi-desktop.exe");
const installer = path.join(repoRoot, `target/release/bundle/nsis/Fleqi_${version}_x64-setup.exe`);
const artifacts = path.join(repoRoot, "tests/.artifacts/package");
mkdirSync(artifacts, { recursive: true });
const hash = (file) => crypto.createHash("sha256").update(readFileSync(file)).digest("hex");
const bytes = readFileSync(binary);
// Tauri 2.11 marks the installer payload NSS, then restores the build output to UNK.
// Normalize only that unique, documented marker; every other byte must match.
const marker = Buffer.from("__TAURI_BUNDLE_TYPE_VAR_UNK");
const markerOffset = bytes.indexOf(marker);
if (markerOffset < 0 || bytes.indexOf(marker, markerOffset + 1) >= 0) throw new Error("Unexpected Tauri bundle marker layout");
const packaged = Buffer.from(bytes);
Buffer.from("__TAURI_BUNDLE_TYPE_VAR_NSS").copy(packaged, markerOffset);
const packagedHash = crypto.createHash("sha256").update(packaged).digest("hex");
const pe = bytes.readUInt32LE(0x3c);
if (bytes.toString("ascii", pe, pe + 4) !== "PE\0\0" || bytes.readUInt16LE(pe + 4) !== 0x8664 || bytes.readUInt16LE(pe + 24 + 68) !== 2) throw new Error("Expected an x64 Windows GUI PE binary");
if (bytes.includes(Buffer.from("tauri_plugin_wdio_webdriver")) || bytes.includes(Buffer.from("wdio-webdriver:"))) throw new Error("Release binary contains the test driver");
const report = { platform: "windows", version, installer, sha256: hash(installer), binarySha256: hash(binary), packagedBinarySha256: packagedHash, checkedAt: new Date().toISOString(), checks: ["PE32+ x64 GUI", "no embedded test driver"] };
function ps(script) {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", windowsHide: true, timeout: 20_000 });
  if (result.status !== 0) throw new Error(result.stderr || result.error?.message || `PowerShell exited ${result.status}`);
  return result.stdout.trim();
}
const existing = ps("@('HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Fleqi','HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Fleqi') | Where-Object { Test-Path -LiteralPath $_ }");
if (existing) throw new Error(`已存在 Fleqi 安装记录，未覆盖用户安装：${existing}`);
if (ps("Get-CimInstance Win32_Process -Filter \"name = 'fleqi-desktop.exe'\" | Select-Object -ExpandProperty ProcessId")) throw new Error("已有 Fleqi 进程，未中断用户应用");
const installDir = mkdtempSync(path.join(artifacts, "windows-install-"));
const installedBinary = path.join(installDir, "fleqi-desktop.exe");
function install() {
  const result = spawnSync(installer, ["/S", "/NS", `/D=${installDir}`], { windowsHide: true, timeout: 120_000 });
  if (result.status !== 0 || !existsSync(installedBinary)) throw new Error(`NSIS install failed: ${result.status} ${result.error ?? ""}`);
  if (hash(installedBinary) !== packagedHash) throw new Error("Installed binary differs from the expected NSIS payload");
}
let child;
try {
  install(); report.checks.push("fresh NSIS install and binary hash");
  writeFileSync(installedBinary, "owned installer replacement fixture");
  install(); report.checks.push("NSIS reinstall");
  const reserve = net.createServer();
  await new Promise((resolve) => reserve.listen(0, "127.0.0.1", resolve));
  const port = reserve.address().port;
  await new Promise((resolve) => reserve.close(resolve));
  child = spawn(installedBinary, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`, WEBVIEW2_USER_DATA_FOLDER: path.join(installDir, "webview-smoke") }, windowsHide: false, stdio: "ignore" });
  const exited = new Promise((resolve) => child.once("exit", resolve));
  let target;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((entry) => entry.type === "page" && entry.url.startsWith("http://tauri.localhost/")); } catch { /* WebView2 is starting. */ }
    if (target) break;
    if (child.exitCode !== null) throw new Error(`Installed app exited ${child.exitCode}`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  if (!target) throw new Error("Installed app did not open its own WebView2 page");
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once: true }); socket.addEventListener("error", reject, { once: true }); });
  let sequence = 0;
  function evaluate(expression) {
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { socket.removeEventListener("message", receive); reject(new Error("CDP evaluation timed out")); }, 15_000);
      const receive = (event) => {
        const response = JSON.parse(event.data);
        if (response.id !== id) return;
        clearTimeout(timeout); socket.removeEventListener("message", receive);
        if (response.error || response.result?.exceptionDetails) reject(new Error(JSON.stringify(response.error ?? response.result.exceptionDetails)));
        else resolve(response.result.result.value);
      };
      socket.addEventListener("message", receive);
      socket.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
    });
  }
  let boot;
  for (let attempt = 0; attempt < 50; attempt++) {
    boot = await evaluate("window.__TAURI_INTERNALS__ ? window.__TAURI_INTERNALS__.invoke('app_bootstrap') : null");
    if (boot?.hostState === "ready") break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (boot?.hostState !== "ready" || boot.buildInfo.targetOs !== "windows" || boot.buildInfo.version !== version || boot.buildInfo.buildProfile !== "release" || boot.storage.state !== "ready") {
    report.startupPage = await evaluate("({url:location.href, title:document.title, text:document.body?.innerText?.slice(0,500)})");
    throw new Error(`Installed app failed bootstrap: ${JSON.stringify(report.startupPage)}`);
  }
  report.bootstrap = { buildInfo: boot.buildInfo, hostState: boot.hostState, storage: boot.storage.state };
  report.checks.push("installed release ready with real IPC");
  await evaluate("setTimeout(() => window.__TAURI_INTERNALS__.invoke('app_quit'), 100); true");
  socket.close();
  let exitTimer;
  try { await Promise.race([exited, new Promise((_, reject) => { exitTimer = setTimeout(() => reject(new Error("Installed app did not quit")), 10_000); })]); }
  finally { clearTimeout(exitTimer); }
  report.checks.push("graceful app quit");
} finally {
  if (child && child.exitCode === null) child.kill();
  const uninstaller = path.join(installDir, "uninstall.exe");
  if (existsSync(uninstaller)) {
    const result = spawnSync(uninstaller, ["/S", `_?=${installDir}`], { windowsHide: true, timeout: 60_000 });
    if (result.status !== 0) throw new Error(`NSIS uninstall failed ${result.status}`);
    if (existsSync(installedBinary)) throw new Error("NSIS did not remove its installed binary");
    report.checks.push("NSIS uninstall; app data preserved");
  }
  writeFileSync(path.join(artifacts, "windows-package-evidence.json"), JSON.stringify(report, null, 2) + "\n");
}
console.log(JSON.stringify(report, null, 2));
