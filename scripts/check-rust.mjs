#!/usr/bin/env node
/**
 * check:rust：以 rustup 分发的实际工具链（rust-toolchain.toml）运行
 * fmt 检查、Clippy（workspace/all-targets/-D warnings）与 workspace 测试（--locked）。
 */
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import path from "node:path";
import { repoRoot } from "./lib/contract.mjs";

const env = { ...process.env, PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}` };
const steps = [
  ["cargo", ["fmt", "--all", "--check"]],
  ["cargo", ["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"]],
  ["cargo", ["test", "--workspace", "--locked"]],
];

for (const [cmd, args] of steps) {
  console.log(`\n$ ${cmd} ${args.join(" ")}`);
  const result = spawnSync(cmd, args, { cwd: repoRoot, env, stdio: "inherit" });
  if (result.status !== 0) {
    console.error(`check:rust 失败于 ${cmd} ${args.join(" ")}`);
    process.exit(result.status ?? 1);
  }
}
console.log("\ncheck:rust 通过。");
