#!/usr/bin/env node
// Keep Tauri's Cargo invocation on the same pinned rustup toolchain as checks.
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import path from "node:path";
const child = spawn("pnpm", ["--filter", "fleqi-desktop", "exec", "tauri", ...process.argv.slice(2)], {
  stdio: "inherit",
  env: { ...process.env, PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}` },
});
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => child.kill(signal));
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
child.on("exit", (code) => { process.exitCode = code ?? 1; });
