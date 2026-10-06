import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { repoRoot } from "./lib/contract.mjs";
import { invocation } from "./lib/process.mjs";

const artifacts = path.join(repoRoot, "tests/.artifacts/desktop");
mkdirSync(artifacts, { recursive: true });
const data = mkdtempSync(path.join(artifacts, "windows-data-"));
const fixtures = path.join(data, "fixtures");
for (const name of ["A 中文", "B '空格'"]) {
  mkdirSync(path.join(fixtures, name), { recursive: true });
  writeFileSync(path.join(fixtures, name, "one.txt"), "one");
  writeFileSync(path.join(fixtures, name, "two.txt"), "two");
}
const server = http.createServer((request, response) => {
  request.resume();
  request.on("end", () => {
    if (request.url?.endsWith("/models")) {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ data: [{ id: "windows-fixture-model" }] }));
      return;
    }
    const plan = JSON.stringify({ scripts: ["[IO.File]::WriteAllText((Join-Path $PWD 'ai-result.txt'), 'verified'); Write-Output 'verified'"], effects: ["create"], previewComplete: true });
    response.writeHead(200, { "Content-Type": "text/event-stream" });
    response.end(`data: ${JSON.stringify({ choices: [{ delta: { content: plan } }] })}\n\ndata: [DONE]\n\n`);
  });
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
const env = { ...process.env, FLEQI_TEST_DATA_DIR: data, FLEQI_WINDOWS_FIXTURE_DIR: fixtures, FLEQI_TEST_MODEL_URL: `http://127.0.0.1:${address.port}/v1` };
function run(args) {
  const [command, argv] = invocation("pnpm", args);
  return new Promise((resolve, reject) => {
    const child = spawn(command, argv, { cwd: repoRoot, env, stdio: "inherit", windowsHide: true });
    child.on("error", reject);
    child.on("exit", (code) => code === 0 ? resolve() : reject(new Error(`Windows desktop check exited ${code}`)));
  });
}
try {
  if (!process.argv.includes("--skip-build")) await run(["--filter", "fleqi-desktop", "run", "build:test"]);
  for (const phase of ["initial", "restart"]) {
    env.FLEQI_TEST_PHASE = phase;
    await run(["--filter", "fleqi-desktop-test", "exec", "wdio", "run", "wdio.windows.conf.ts"]);
  }
} finally {
  server.close();
  const cleanup = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(repoRoot, "tests/desktop/lib/windows-explorer.ps1"), "-Action", "close"], { env, encoding: "utf8", windowsHide: true });
  if (cleanup.status !== 0) console.error(cleanup.stderr);
}
