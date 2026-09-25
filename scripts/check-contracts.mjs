#!/usr/bin/env node
/**
 * P0-CONTRACT-001 · check:contracts
 * 把 Rust DTO 经 ts-rs 导出到临时目录，与提交的 packages/contracts/src/bindings
 * 逐文件逐字节比对；另核对 Cargo workspace、根 package.json 与 contracts 包的版本一致。
 * 生成（contracts:regen）与校验分离；普通 cargo test 不改生成文件。
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { fail, ok, repoRoot } from "./lib/contract.mjs";

export const BINDINGS_REL = "packages/contracts/src/bindings";

export function exportBindings(exportDir) {
  const env = {
    ...process.env,
    PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}`,
    TS_RS_EXPORT_DIR: exportDir,
  };
  const result = spawnSync(
    "cargo",
    ["test", "-p", "fleqi-application", "--features", "export-contracts", "--locked", "--test", "export_contracts", "--quiet"],
    { cwd: repoRoot, env, stdio: ["ignore", "pipe", "pipe"], encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(`契约导出失败：\n${result.stdout}\n${result.stderr}`);
  }
  for (const file of listFiles(path.join(exportDir, BINDINGS_REL))) {
    const destination = path.join(exportDir, BINDINGS_REL, file);
    writeFileSync(destination, readFileSync(destination, "utf8").replace(/[\t ]+$/gm, ""));
  }
}

function listFiles(dir) {
  return existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith(".ts")).sort() : [];
}

/** 绑定类型名列表（文件名去扩展名，字母序）。 */
export function listBindings(dir) {
  return listFiles(dir).map((f) => f.replace(/\.ts$/, ""));
}

/** 由绑定列表生成 src/index.ts（确定性输出，便于比对）。 */
export function renderIndex(names) {
  const lines = [
    "/**",
    " * @fleqi/contracts 公共入口（由 pnpm contracts:regen 生成，不要手改）。",
    " * 所有类型来自 crates/fleqi-application 的 DTO（ts-rs 生成，见 ./bindings）。",
    " * UI 与宿主适配器只从这里引用跨 IPC 类型，避免手写第二份状态定义。",
    " */",
    ...names.map((name) => `export type { ${name} } from "./bindings/${name}";`),
    "",
  ];
  return lines.join("\n");
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  let failures = 0;
  console.log("check:contracts · DTO → TypeScript 契约一致性\n");

  const cargoToml = readFileSync(path.join(repoRoot, "Cargo.toml"), "utf8");
  const workspaceVersion = cargoToml.match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
  const rootVersion = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8")).version;
  const contractsVersion = JSON.parse(readFileSync(path.join(repoRoot, "packages/contracts/package.json"), "utf8")).version;
  if (!workspaceVersion) failures += fail("Cargo.toml 缺少 workspace.package.version");
  if (workspaceVersion !== rootVersion || rootVersion !== contractsVersion) {
    failures += fail(`版本不一致：Cargo ${workspaceVersion} / 根 package.json ${rootVersion} / contracts ${contractsVersion}`);
  } else {
    ok(`版本一致：${workspaceVersion}`);
  }
  if (!/^\d+\.\d+\.\d+$/.test(rootVersion ?? "")) failures += fail(`应用版本应为 x.y.z 数字段，Beta 渠道由 stage 标识：${rootVersion}`);
  const bundleVersion = JSON.parse(readFileSync(path.join(repoRoot, "apps/desktop/src-tauri/tauri.conf.json"), "utf8")).version;
  if (bundleVersion !== rootVersion) failures += fail(`Tauri 版本 ${bundleVersion} 与 workspace ${rootVersion} 不一致`);

  const tmp = mkdtempSync(path.join(tmpdir(), "fleqi-contracts-"));
  try {
    exportBindings(tmp);
    const generatedDir = path.join(tmp, BINDINGS_REL);
    const committedDir = path.join(repoRoot, BINDINGS_REL);
    const generated = listFiles(generatedDir);
    const committed = listFiles(committedDir);
    if (generated.length === 0) failures += fail("导出未产生任何绑定文件");
    for (const file of generated) {
      if (!committed.includes(file)) {
        failures += fail(`缺少已提交绑定：${file}（运行 pnpm contracts:regen）`);
        continue;
      }
      const a = readFileSync(path.join(generatedDir, file));
      const b = readFileSync(path.join(committedDir, file));
      if (!a.equals(b)) failures += fail(`绑定内容不一致：${file}（运行 pnpm contracts:regen）`);
    }
    for (const file of committed) {
      if (!generated.includes(file)) failures += fail(`多余的已提交绑定：${file}（源 DTO 已删除？）`);
    }
    const expectedIndex = renderIndex(listBindings(generatedDir));
    const committedIndex = readFileSync(path.join(repoRoot, "packages/contracts/src/index.ts"), "utf8");
    if (expectedIndex !== committedIndex) failures += fail("packages/contracts/src/index.ts 与绑定列表不一致（运行 pnpm contracts:regen）");
    if (failures === 0) ok(`${generated.length} 个绑定文件与生成结果逐字节一致，index.ts 完整导出`);
  } catch (error) {
    failures += fail(error.message);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }

  if (existsSync(path.join(repoRoot, "crates/fleqi-application/bindings"))) {
    failures += fail("crates/fleqi-application/bindings 存在：普通测试不得生成绑定（检查 #[ts(export)] 使用）");
  }

  console.log("");
  if (failures > 0) {
    console.error(`check:contracts 失败：${failures} 项`);
    process.exit(1);
  }
  console.log("check:contracts 通过。");
}
