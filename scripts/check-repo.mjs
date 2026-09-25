#!/usr/bin/env node
/**
 * P0-REPO-001 · check:repo
 * 校验仓库根与远程、单 main 分支、忽略边界（视频/依赖/构建产物/缓存/工具记录）
 * 与参考源跟踪状态；确保没有构建产物被提交。
 */
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fail, ok, repoRoot } from "./lib/contract.mjs";

function git(args, { allowFail = false } = {}) {
  try {
    return execFileSync("git", ["-c", "core.quotepath=false", ...args], { cwd: repoRoot, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }).trim();
  } catch (error) {
    if (allowFail) return null;
    throw error;
  }
}

let failures = 0;
console.log("check:repo · 仓库边界\n");

const toplevel = git(["rev-parse", "--show-toplevel"]);
if (toplevel !== repoRoot) failures += fail(`仓库根 ${toplevel} ≠ 工程根 ${repoRoot}`);
else ok(`仓库根：${path.basename(repoRoot)}`);

const origin = git(["remote", "get-url", "origin"], { allowFail: true });
if (!origin || !/github\.com[/:]Fleqi-App\/fleqi(\.git)?$/.test(origin)) failures += fail(`origin 异常：${origin}`);
else ok(`origin：${origin}`);

const localBranches = git(["branch", "--format=%(refname:short)"]).split("\n").filter(Boolean);
if (localBranches.length !== 1 || localBranches[0] !== "main") failures += fail(`本地分支应为 [main]，实际 [${localBranches.join(", ")}]`);
else ok("本地分支：main（单分支）");

const remoteBranches = git(["ls-remote", "--heads", "origin"], { allowFail: true })?.split("\n").filter((l) => l.includes("refs/heads/")).map((l) => l.split("refs/heads/")[1]) ?? null;
if (remoteBranches == null) {
  console.log("  ! 远程分支无法查询（离线？），跳过");
} else if (remoteBranches.length !== 1 || remoteBranches[0] !== "main") {
  failures += fail(`远程分支应为 [main]，实际 [${remoteBranches.join(", ")}]`);
} else {
  ok("远程分支：main（单分支）");
}

const VIDEO = "docs/交互设计参考.mov";
const videoIgnored = git(["check-ignore", VIDEO], { allowFail: true });
if (!videoIgnored) failures += fail(`${VIDEO} 未被忽略（仅本机保留）`);
else ok("本机视频被忽略");

const IGNORED_MUST = ["node_modules", "target", "dist"];
for (const name of IGNORED_MUST) {
  const probe = path.join(repoRoot, name, ".fleqi-probe");
  const hit = git(["check-ignore", probe], { allowFail: true });
  if (!hit) failures += fail(`构建产物目录 ${name}/ 未被忽略`);
}
ok("node_modules/target/dist 均被忽略");

const trackedFiles = git(["ls-files"]).split("\n").filter(Boolean);
const artifactPattern = /(^|\/)(node_modules|target|dist|\.cache|coverage|logs|playwright-report|test-results|tests\/\.artifacts)(\/|$)|\.tsbuildinfo$|\.DS_Store$/;
const trackedArtifacts = trackedFiles.filter((f) => artifactPattern.test(f));
if (trackedArtifacts.length > 0) {
  for (const file of trackedArtifacts.slice(0, 10)) failures += fail(`构建产物被提交：${file}`);
} else {
  ok("无构建产物/缓存被提交");
}

const REFERENCE_MUST_TRACK = [
  "Web APP/references/workspace.png",
  "Web APP/references/settings-general.png",
  "Web APP/docs/previews/core-workspace.png",
  "Web APP/docs/previews/core-settings-general.png",
  "Icon/exports/Fleqi-iOS-Default-1024@1x.png",
  "docs/ui-baselines/approved-1.png",
  "docs/ui-baselines/approved-2.png",
];
for (const file of REFERENCE_MUST_TRACK) {
  if (!existsSync(path.join(repoRoot, file)) || !trackedFiles.includes(file)) failures += fail(`参考源未被跟踪：${file}`);
}
ok(`参考源（截图/预览/图标导出/留档）被跟踪：${REFERENCE_MUST_TRACK.length} 项`);

console.log("");
if (failures > 0) {
  console.error(`check:repo 失败：${failures} 项`);
  process.exit(1);
}
console.log("check:repo 通过。");
