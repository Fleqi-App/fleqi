import { existsSync } from "node:fs";

/** pnpm supplies its real entrypoint to scripts; Node cannot spawn a .cmd shim. */
export function invocation(command, args = []) {
  if (process.platform === "win32" && command === "pnpm") {
    const entry = process.env.npm_execpath;
    if (!entry || !existsSync(entry)) throw new Error("请通过 pnpm 运行此脚本，以定位项目使用的包管理器。");
    return /\.exe$/i.test(entry)
      ? [entry, args]
      : [process.execPath, [entry, ...args]];
  }
  return [command, args];
}
