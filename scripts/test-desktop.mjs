#!/usr/bin/env node
/**
 * P0-DESKTOP-001 / M1.6 · pnpm test:desktop
 * 1) 用 desktop-test feature + tauri.test.conf.json 构建加载打包本地 UI 的原生测试构建（不打 bundle）；
 * 2) 运行三次 WebdriverIO（embedded 驱动）：run-a（启动/自检/上下文 + 多窗口设置，spec 文件并行），
 *    run-a2（M4 会话编排串行运行）、run-b（新进程，验证重启恢复），三次共用同一独立数据目录；
 * 3) 每次运行后核对宿主进程已退出，合并启动/退出证据到 tests/.artifacts/desktop/lifecycle-<run>.json。
 * 普通包（pnpm tauri build）不含测试驱动。
 */
import { execFileSync, spawn, spawnSync } from "node:child_process";
import http from "node:http";
import crypto from "node:crypto";
import { createReadStream, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { repoRoot } from "./lib/contract.mjs";

const binary = path.join(repoRoot, "target", "debug", "fleqi-desktop");
const artifactsDir = path.join(repoRoot, "tests", ".artifacts", "desktop");
// 每次运行使用全新的独立数据目录与测试凭据命名空间（desktop-test feature 读取该变量）。
const dataDir = path.join(artifactsDir, `desktop-data-${new Date().toISOString().replace(/[:.]/g, "-")}`);
// AC-FLOW-001 的独立"新安装"数据目录（其余运行共享 dataDir）。
const dataDir001 = path.join(artifactsDir, `desktop-data-fresh-001-${new Date().toISOString().replace(/[:.]/g, "-")}`);
const baseEnv = {
  ...process.env,
  PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}`,
  FLEQI_TEST_DATA_DIR: dataDir,
};

function run(cmd, args, { allowFail = false, env = baseEnv } = {}) {
  console.log(`\n$ ${cmd} ${args.join(" ")}`);
  const result = spawnSync(cmd, args, { cwd: repoRoot, env, stdio: "inherit" });
  if (result.status !== 0 && !allowFail) {
    console.error(`test:desktop 失败于 ${cmd} ${args.join(" ")}`);
    process.exit(result.status ?? 1);
  }
  return result.status ?? 1;
}

function runningPids() {
  try {
    return execFileSync("pgrep", ["-f", "--", binary], { encoding: "utf8" }).split("\n").filter(Boolean).map(Number);
  } catch {
    return [];
  }
}

function readJson(name) {
  const file = path.join(artifactsDir, name);
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : {};
}

function waitForExit() {
  const deadline = Date.now() + 15_000;
  let remaining = runningPids();
  while (remaining.length > 0 && Date.now() < deadline) {
    execFileSync("sleep", ["0.25"]);
    remaining = runningPids();
  }
  return remaining;
}

// M2 收口 · Finder 几何驱动器：run-a2 期间由 spec 09 经标志文件启停，
// 以 osascript 真实移动 Finder 前窗（等价拖动；用户已许可的自动驱动）。
// 驱动前先打开自己的 /tmp 临时文件夹窗口，结束后按窗口 id 关闭并清理；
// 不移动用户已有 Finder 窗口。
const GEOMETRY_FLAG = "/tmp/fleqi-geom-enabled";
const GEOMETRY_OPEN =
  'on run argv\ntell application "Finder" to make new Finder window to (POSIX file (item 1 of argv) as alias)\nend run';
const GEOMETRY_WINDOW_ID =
  'tell application "Finder" to tell front window to get id';
const GEOMETRY_CLOSE =
  'on run argv\ntell application "Finder" to close window id ((item 1 of argv) as integer)\nend run';
const GEOMETRY_MOVES = [
  'tell application "Finder" to set bounds of front window to {80, 80, 680, 460}',
  'tell application "Finder" to set bounds of front window to {150, 150, 750, 530}',
];

/** 异步 wdio 运行：不阻塞事件循环，几何驱动器的定时器才能在运行期间触发。 */
function runWdio(args, env = baseEnv) {
  return new Promise((resolve) => {
    console.log(`\n$ pnpm ${args.join(" ")}`);
    const child = spawn("pnpm", args, { cwd: repoRoot, env, stdio: "inherit" });
    child.on("exit", (code) => resolve(code ?? 1));
    child.on("error", () => resolve(1));
  });
}

async function startFinderMover() {
  let windowId = null;
  let folder = null;
  let index = 0;
  const controller = { stop: false };
  const worker = (async () => {
    while (!controller.stop) {
      await new Promise((resolve) => setTimeout(resolve, 2500));
      if (controller.stop) break;
      if (!existsSync(GEOMETRY_FLAG)) continue;
      try {
        // 首次置位才打开自己的 Finder 窗口（避免污染 08 运行期间的上下文）。
        if (windowId === null) {
          folder = path.join("/tmp", `fleqi-geom-${Date.now()}`);
          mkdirSync(folder, { recursive: true });
          execFileSync("osascript", ["-e", GEOMETRY_OPEN, folder], { encoding: "utf8" });
          windowId = execFileSync("osascript", ["-e", GEOMETRY_WINDOW_ID], { encoding: "utf8" }).trim();
          console.log(`[mover] 已打开 Finder 窗口 ${windowId}（${folder}）`);
        }
        execFileSync("osascript", ["-e", GEOMETRY_MOVES[index % GEOMETRY_MOVES.length]], { encoding: "utf8" });
        console.log(`[mover] 第 ${index + 1} 次移动前窗`);
        index += 1;
      } catch (error) {
        console.error(`[mover] 驱动失败：${error?.message ?? error}`);
      }
    }
  })();
  console.log("[mover] 几何驱动器已启动（等待标志文件）");
  return {
    controller,
    async stop() {
      controller.stop = true;
      await worker;
      try {
        const listing = execFileSync(
          "osascript",
          ["-e", 'tell application "Finder"\nset out to ""\nrepeat with w in Finder windows\nset out to out & (id of w as text) & ":" & (POSIX path of (target of w as alias)) & " | "\nend repeat\nreturn out\nend tell'],
          { encoding: "utf8" },
        );
        console.log(`[driver-005] 停止前 Finder 窗口：${listing.trim()}`);
      } catch (error) {
        console.error(`[driver-005] 窗口清单失败：${error?.message ?? error}`);
      }
      if (windowId !== null) {
        try {
          execFileSync("osascript", ["-e", GEOMETRY_CLOSE, windowId], { encoding: "utf8" });
        } catch {
          // 窗口可能已被用户关闭：清理继续。
        }
      }
      if (folder !== null) rmSync(folder, { recursive: true, force: true });
      rmSync(GEOMETRY_FLAG, { force: true });
    },
  };
}

// M4 贴附证据（run-a2-04）：打开自建 Finder 目标窗，把其 bounds 写入标志文件；
// 几何断言在 spec 04 内完成（webview 的 window.screenX/outerWidth 对照 Finder
// bounds，不经过 System Events——后者在频繁重启应用的机器上视图不稳定）。
const ATTACH_FINDER_FILE = "/tmp/fleqi-attach-finder.json";
const ATTACH_FINDER_GEOMETRY =
  'tell application "Finder"\n' +
  'if (count of Finder windows) is 0 then return "no-finder-window"\n' +
  "set b to bounds of Finder window 1\n" +
  "return (item 1 of b as text) & \",\" & (item 2 of b as text) & \",\" & (item 3 of b as text) & \",\" & (item 4 of b as text)\n" +
  "end tell";

function startAttachTarget() {
  const controller = { stop: false };
  let folder = null;
  let windowId = null;
  try {
    // 打开自己的 Finder 窗口作为贴附目标（结束时关闭并清理，不触碰用户窗口）。
    folder = path.join("/tmp", `fleqi-attach-${Date.now()}`);
    mkdirSync(folder, { recursive: true });
    execFileSync("osascript", ["-e", FINDER_OPEN_SCRIPT, folder], { encoding: "utf8" });
    windowId = execFileSync("osascript", ["-e", GEOMETRY_WINDOW_ID], { encoding: "utf8" }).trim();
    // Finder 新窗会在打开后恢复/级联位置；立即采样可能记录动画前坐标。
    // 只固定本次自建窗口，避免把窗口恢复动画误判成 App 贴附失败。
    execFileSync("osascript", ["-e", 'on run argv\ntell application "Finder" to set bounds of window id ((item 1 of argv) as integer) to {80, 80, 1000, 544}\nend run', windowId]);
    let stableSamples = 0;
    for (let attempt = 0; attempt < 20 && stableSamples < 3; attempt += 1) {
      execFileSync("sleep", ["0.15"]);
      const sample = execFileSync("osascript", ["-e", ATTACH_FINDER_GEOMETRY], { encoding: "utf8" }).trim();
      stableSamples = sample === "80,80,1000,544" ? stableSamples + 1 : 0;
    }
    if (stableSamples < 3) throw new Error("自建 Finder 窗口未稳定到测试坐标");
    const bounds = execFileSync("osascript", ["-e", ATTACH_FINDER_GEOMETRY], { encoding: "utf8" }).trim();
    const [x1, y1, x2, y2] = bounds.split(",").map(Number);
    if (![x1, y1, x2, y2].every(Number.isFinite)) throw new Error(`Finder 目标窗 bounds 非法：${bounds}`);
    writeFileSync(ATTACH_FINDER_FILE, JSON.stringify({ folder, windowId, x1, y1, x2, y2, sampledAt: new Date().toISOString() }));
    console.log(`[attach-target] Finder 目标窗 ${windowId}：${bounds}`);
  } catch (error) {
    console.error(`[attach-target] 目标窗准备失败：${error?.message ?? error}`);
  }
  return {
    controller,
    async stop() {
      controller.stop = true;
      if (windowId !== null) {
        try {
          execFileSync("osascript", ["-e", GEOMETRY_CLOSE, windowId], { encoding: "utf8" });
        } catch {
          // 窗口可能已被关闭：清理继续。
        }
      }
      if (folder !== null) rmSync(folder, { recursive: true, force: true });
      rmSync(ATTACH_FINDER_FILE, { force: true });
    },
  };
}

// AC-FLOW-001/005 · Finder 事件驱动器（真实 Finder，按用户许可的自动驱动）：
// spec 经标志文件请求事件，驱动器执行后写完成标记。各自独占运行阶段启用，
// 结束时关闭自建窗口并清理 /tmp，不触碰用户已有窗口。
const SWITCH_FLAG = "/tmp/fleqi-001-switch";
const SWITCH_MARKER = "/tmp/fleqi-001-switch-done";
const SEQ_A_FLAG = "/tmp/fleqi-005-open-a";
const SEQ_A_MARKER = "/tmp/fleqi-005-a-done";
const SEQ_FLAG = "/tmp/fleqi-005-sequence";
const SEQ_MARKER = "/tmp/fleqi-005-seq-done";
const FINDER_OPEN_SCRIPT =
  'on run argv\ntell application "Finder" to make new Finder window to (POSIX file (item 1 of argv) as alias)\nend run';
// 打开文件夹并激活 Finder：等价用户点击 Finder 窗口（触发 didActivateApplication，
// 产品的上下文跟随以激活事件为依据）。
const FINDER_OPEN_ACTIVATE_SCRIPT =
  'on run argv\ntell application "Finder"\nmake new Finder window to (POSIX file (item 1 of argv) as alias)\nactivate\nend tell\nend run';
// 焦点切换：先让一个普通应用到前台（Finder 失活），下一次 Finder 激活才能
// 再次触发 didActivateApplication。选 Calculator：系统自带、可脚本激活、无副作用。
const FINDER_QUIT_OTHER_APP_SCRIPT = 'tell application "Calculator" to quit';
const FINDER_FRONT_ID_SCRIPT =
  'tell application "Finder" to tell front window to get id';
const FINDER_CLOSE_SCRIPT =
  'on run argv\ntell application "Finder" to close window id ((item 1 of argv) as integer)\nend run';

function openFinderFolder(folder, activate = false) {
  execFileSync("osascript", ["-e", activate ? FINDER_OPEN_ACTIVATE_SCRIPT : FINDER_OPEN_SCRIPT, folder], { encoding: "utf8" });
  return execFileSync("osascript", ["-e", FINDER_FRONT_ID_SCRIPT], { encoding: "utf8" }).trim();
}

async function startFinderSwitchDriver() {
  const controller = { stop: false };
  const worker = (async () => {
    while (!controller.stop) {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      if (controller.stop || !existsSync(SWITCH_FLAG)) continue;
      try {
        const folder = path.join("/tmp", `fleqi-001-dir-${Date.now()}`);
        mkdirSync(folder, { recursive: true });
        const windowId = openFinderFolder(folder);
        writeFileSync(SWITCH_MARKER, JSON.stringify({ folder, windowId }));
        console.log(`[driver-001] 已切换 Finder 到 ${folder}（窗口 ${windowId}）`);
        return; // 一次性事件：完成后退出轮询。
      } catch (error) {
        console.error(`[driver-001] 驱动失败：${error?.message ?? error}`);
      }
    }
  })();
  return {
    controller,
    async stop() {
      controller.stop = true;
      await worker;
      rmSync(SWITCH_FLAG, { force: true });
      rmSync(SWITCH_MARKER, { force: true });
    },
  };
}

async function startFinderSequenceDriver() {
  const calculatorWasRunning = execFileSync("osascript", ["-e", 'application "Calculator" is running'], { encoding: "utf8" }).trim() === "true";
  const controller = { stop: false };
  const windows = [];
  let base = null;
  let openedB = false;
  const worker = (async () => {
    while (!controller.stop) {
      await new Promise((resolve) => setTimeout(resolve, 1000));
      if (controller.stop) break;
      try {
        // 阶段一：置位 SEQ_A_FLAG → 打开并激活 A（建立上下文与工作目录）。
        if (base === null && existsSync(SEQ_A_FLAG)) {
          base = path.join("/tmp", `fleqi-005-${Date.now()}`);
          for (const name of ["a", "b", "c"]) mkdirSync(path.join(base, name), { recursive: true });
          windows.push(openFinderFolder(path.join(base, "a"), true));
          writeFileSync(SEQ_A_MARKER, JSON.stringify({ a: path.join(base, "a") }));
          rmSync(SEQ_A_FLAG, { force: true });
          console.log(`[driver-005] A 已打开（${path.join(base, "a")}）`);
          continue;
        }
        // 阶段二：置位 SEQ_FLAG → 在 vim 占用期间依次切换 B、C。
        // 每次切换：焦点先给 Fleqi（Finder 失活）再 open+activate Finder ——
        // 产生真实 didActivateApplication 事件（与用户在 App 与 Finder 间点击一致）。
        if (base !== null && !openedB && existsSync(SEQ_FLAG)) {
          rmSync(SEQ_FLAG, { force: true });
          execFileSync("open", ["-a", "Calculator"], { encoding: "utf8" });
          await new Promise((resolve) => setTimeout(resolve, 800));
          windows.push(openFinderFolder(path.join(base, "b"), true));
          await new Promise((resolve) => setTimeout(resolve, 1500));
          execFileSync("open", ["-a", "Calculator"], { encoding: "utf8" });
          await new Promise((resolve) => setTimeout(resolve, 800));
          windows.push(openFinderFolder(path.join(base, "c"), true));
          openedB = true;
          writeFileSync(SEQ_MARKER, JSON.stringify({ b: path.join(base, "b"), c: path.join(base, "c") }));
          console.log(`[driver-005] B→C 序列完成`);
        }
      } catch (error) {
        console.error(`[driver-005] 驱动失败：${error?.message ?? error}`);
      }
    }
  })();
  return {
    controller,
    async stop() {
      controller.stop = true;
      await worker;
      for (const windowId of windows) {
        try {
          execFileSync("osascript", ["-e", FINDER_CLOSE_SCRIPT, windowId], { encoding: "utf8" });
        } catch {
          // 窗口可能已被用户关闭：清理继续。
        }
      }
      rmSync(SEQ_A_FLAG, { force: true });
      rmSync(SEQ_A_MARKER, { force: true });
      rmSync(SEQ_FLAG, { force: true });
      rmSync(SEQ_MARKER, { force: true });
      if (!calculatorWasRunning) spawnSync("osascript", ["-e", FINDER_QUIT_OTHER_APP_SCRIPT], { encoding: "utf8" });
      if (base) rmSync(base, { recursive: true, force: true });
    },
  };
}

// AC-FLOW-012 · 受管工具目录 + 回环下载源：阶段开始前把受管目录清单写入
// 数据目录（tools/managed-catalog.json，宿主启动时装载），并在 runner 进程内
// 启动 127.0.0.1 静态服务提供工具包（合同允许回环来源）。阶段结束关闭服务。
function prepareToolLoopback(sharedDataDir) {
  const work = path.join("/tmp", `fleqi-012-${Date.now()}`);
  const pkgDir = path.join(work, "pkg");
  mkdirSync(path.join(pkgDir, "bin"), { recursive: true });
  writeFileSync(path.join(pkgDir, "bin", "fleqi-demo-tool"), "#!/bin/sh\necho fleqi-demo-tool 1.0\n");
  execFileSync("chmod", ["+x", path.join(pkgDir, "bin", "fleqi-demo-tool")]);
  execFileSync("zip", ["-rq", path.join(work, "tool.zip"), "pkg"], { cwd: work });
  const sha256 = crypto.createHash("sha256").update(readFileSync(path.join(work, "tool.zip"))).digest("hex");
  const server = http.createServer((req, res) => {
    res.writeHead(200, { "Content-Type": "application/zip" });
    createReadStream(path.join(work, "tool.zip")).pipe(res);
  });
  const urlReady = new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve(`http://127.0.0.1:${server.address().port}/tool.zip`));
  });
  const toolsDir = path.join(sharedDataDir, "tools");
  return {
    async url() {
      const url = await urlReady;
      const catalog = [
        {
          id: "fleqi-demo-tool",
          version: "1.0",
          platform: "macos",
          arch: "any",
          executable: "bin/fleqi-demo-tool",
          source: { kind: "managed", url, sha256 },
          license: "MIT",
          capabilities: ["cap.demo"],
          detectionArgs: ["--version"],
        },
      ];
      mkdirSync(toolsDir, { recursive: true });
      writeFileSync(path.join(toolsDir, "managed-catalog.json"), `${JSON.stringify(catalog, null, 2)}\n`);
      console.log(`[tool-012] 回环源 ${url}；受管目录已写入 ${toolsDir}`);
      return url;
    },
    close() {
      return new Promise((resolve) => server.close(resolve));
    },
  };
}

const LIFECYCLE_PARTS = ["lifecycle-launcher.json", "lifecycle-session.json", "lifecycle-session-end.json"];
const LIFECYCLE_OUTPUT = {
  "run-a": "lifecycle-run-a.json",
  "run-a2": "lifecycle-run-a2.json",
  "run-ui": "lifecycle-run-ui.json",
  "run-workflow": "lifecycle-run-workflow.json",
  "run-launch": "lifecycle-run-launch.json",
  "run-a2-04": "lifecycle-run-a2-04.json",
  "run-a2-09": "lifecycle-run-a2-09.json",
  "run-a2-005": "lifecycle-run-a2-005.json",
  "run-a2-012": "lifecycle-run-a2-012.json",
  "run-a1-001": "lifecycle-run-a1-001.json",
  "run-b": "lifecycle-run-b.json",
};

function finalize(runTag, wdioExitCode, runDataDir) {
  const remaining = waitForExit();
  const lifecycle = {
    ...readJson(LIFECYCLE_PARTS[0]),
    ...readJson(LIFECYCLE_PARTS[1]),
    ...readJson(LIFECYCLE_PARTS[2]),
    dataDir: runDataDir,
    finishedAt: new Date().toISOString(),
    wdioExitCode,
    pidsAfterExit: remaining,
    exitClean: remaining.length === 0,
  };
  writeFileSync(path.join(artifactsDir, LIFECYCLE_OUTPUT[runTag]), `${JSON.stringify(lifecycle, null, 2)}\n`);
  console.log(`\n[${runTag}] 启动/退出证据：${JSON.stringify(lifecycle)}`);
  if (!lifecycle.exitClean) {
    console.error(`[${runTag}] 宿主进程未退出：${remaining.join(", ")}`);
    for (const pid of remaining) spawnSync("kill", [String(pid)]);
    return false;
  }
  if (!lifecycle.pidsDuringSession?.length) {
    console.error(`[${runTag}] 会话期间未观察到宿主进程（启动证据缺失）。`);
    return false;
  }
  return wdioExitCode === 0;
}

if (process.platform === "win32") {
  await import("./test-desktop-windows.mjs");
  process.exit(0);
}
if (process.platform !== "darwin") {
  console.error("test:desktop 目前只在 macOS 运行（首发平台）。");
  process.exit(1);
}

const guiSession = spawnSync("swift", [path.join(repoRoot, "scripts/lib/gui-session.swift")], { encoding: "utf8" });
if (guiSession.status !== 0) {
  console.error("原生 UI 验收需要已解锁的 Mac 桌面；当前锁定或无法读取 GUI 会话。请解锁后重跑。");
  process.exit(1);
}

for (const stale of [...LIFECYCLE_PARTS, ...Object.values(LIFECYCLE_OUTPUT)]) rmSync(path.join(artifactsDir, stale), { force: true });

if (!process.argv.includes("--skip-build")) {
  run("pnpm", ["--filter", "fleqi-desktop", "run", "build:test"]);
}
// AC-FLOW-001 需要"新安装"语义：其 wdio 运行必须是本数据目录上的第一个运行。
if (!existsSync(binary)) {
  console.error(`测试构建不存在：${binary}`);
  process.exit(1);
}
if (runningPids().length > 0) {
  console.error(`已有测试构建进程在运行：${runningPids().join(", ")}，请先结束。`);
  process.exit(1);
}

const runs = [
  // run-a 的各 spec 文件由 wdio 并行 worker 执行（02/07 已按跨 worker 版本竞争设计）；
  // surface/会话编排断言依赖独占状态：08/04/09 各自独占一个串行 wdio 运行
  //（同一 wdio 调用的多 spec 会并行 worker，设置/热键写入互踩）。
  { tag: "run-a", specs: ["specs/01-bootstrap.spec.ts", "specs/02-settings.spec.ts", "specs/05-m3.spec.ts", "specs/06-m3-planning.spec.ts", "specs/07-m4-flows.spec.ts"] },
  { tag: "run-a2", specs: ["specs/08-m4-session.spec.ts"] },
  { tag: "run-a2-04", specs: ["specs/04-m2.spec.ts"] },
  { tag: "run-a2-09", specs: ["specs/09-m2-geometry.spec.ts"] },
  { tag: "run-a2-005", specs: ["specs/11-ac-flow-005.spec.ts"] },
  { tag: "run-a2-012", specs: ["specs/12-ac-flow-012.spec.ts"] },
  { tag: "run-a1-001", specs: ["specs/10-ac-flow-001.spec.ts"] },
  { tag: "run-ui", specs: ["specs/13-ui-regressions.spec.ts"] },
  { tag: "run-workflow", specs: ["specs/14-workflow.spec.ts"] },
  { tag: "run-launch", specs: ["specs/15-launch-conflict.spec.ts"] },
  { tag: "run-b", specs: ["specs/03-restart.spec.ts"] },
];
// AC-FLOW-001 需要"新安装"语义：它使用独立的全新数据目录，不与其它运行共享。
const only = process.env.FLEQI_ONLY ?? null;
const runsToExecute = only ? runs.filter((entry) => entry.tag === only) : runs;
let allOk = true;
let mover = null;
let switchDriver = null;
let seqDriver = null;
let toolServer = null;
let attachTarget = null;
for (const { tag, specs } of runsToExecute) {
  for (const part of LIFECYCLE_PARTS) rmSync(path.join(artifactsDir, part), { force: true });
  const args = ["--filter", "fleqi-desktop-test", "exec", "wdio", "run", "wdio.conf.ts"];
  for (const spec of specs) args.push("--spec", spec);
  if (process.env.FLEQI_GREP) args.push("--mochaOpts.grep", process.env.FLEQI_GREP);
  if (tag === "run-a2-09") mover = await startFinderMover();
  if (tag === "run-a1-001") switchDriver = await startFinderSwitchDriver();
  if (tag === "run-a2-005") seqDriver = await startFinderSequenceDriver();
  if (["run-a", "run-a2", "run-a2-04", "run-a2-012", "run-ui", "run-workflow", "run-launch"].includes(tag)) attachTarget = startAttachTarget();
  if (tag === "run-a2-012") {
    toolServer = prepareToolLoopback(dataDir);
    await toolServer.url();
  }
  const env = ["run-ui", "run-workflow", "run-launch"].includes(tag) ? { ...baseEnv, FLEQI_TEST_DATA_DIR: `${dataDir}-${tag.slice(4)}` } : tag === "run-a1-001" ? { ...baseEnv, FLEQI_TEST_DATA_DIR: dataDir001 } : baseEnv;
  const code = await runWdio(args, env);
  const runOk = finalize(tag, code, env.FLEQI_TEST_DATA_DIR);
  if (attachTarget) {
    await attachTarget.stop();
    attachTarget = null;
  }
  if (mover) {
    await mover.stop();
    mover = null;
  }
  if (switchDriver) {
    await switchDriver.stop();
    switchDriver = null;
  }
  if (seqDriver) {
    await seqDriver.stop();
    seqDriver = null;
  }
  if (toolServer) {
    await toolServer.close();
    toolServer = null;
  }
  if (!runOk) allOk = false;
  if (!allOk) break;
}

if (!allOk) {
  console.error("test:desktop 失败：见上方各运行的结果。");
  process.exit(1);
}
console.log("\ntest:desktop 通过；证据见 tests/.artifacts/desktop/。");
