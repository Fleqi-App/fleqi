#!/usr/bin/env node
/**
 * contracts:regen：显式重新生成 packages/contracts/src/bindings（提交产物）与 src/index.ts 入口。
 * 与 check:contracts 分离；只有 DTO 变化时运行，并把结果一并提交。
 */
import { writeFileSync } from "node:fs";
import path from "node:path";
import { BINDINGS_REL, exportBindings, listBindings, renderIndex } from "./check-contracts.mjs";
import { repoRoot } from "./lib/contract.mjs";

exportBindings(repoRoot);
const names = listBindings(path.join(repoRoot, BINDINGS_REL));
writeFileSync(path.join(repoRoot, "packages/contracts/src/index.ts"), renderIndex(names));
console.log(`已重新生成 ${BINDINGS_REL}/*.ts（${names.length} 个）与 src/index.ts；请运行 pnpm check:contracts 并提交。`);
