import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'src');
const problems = [];
function visit(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) visit(file);
    else if (/\.[cm]?[jt]sx?$/.test(entry.name)) {
      const text = readFileSync(file, 'utf8');
      if (/@tauri-apps\/|FleqiController|\bfetch\s*\(|__TAURI/.test(text)) problems.push(`${path.relative(root, file)} couples UI to a native/network backend`);
      if (directory.includes(`${path.sep}core`) && /localStorage|sessionStorage|window\.location|from\s+['"].*\/preview/.test(text)) problems.push(`${path.relative(root, file)} owns preview/storage concerns`);
    }
  }
}
visit(source);
for (const [file, other] of [['WorkspaceUI.tsx', 'GeneralSettingsUI'], ['GeneralSettingsUI.tsx', 'WorkspaceUI']]) {
  if (new RegExp(`from\\s+['"].*${other}`).test(readFileSync(path.join(source, 'core', file), 'utf8'))) problems.push(`${file} imports the other screen directly`);
}
const exports = [...readFileSync(path.join(root, 'CoreUI.tsx'), 'utf8').matchAll(/export\s+\{([^}]+)\}/g)].flatMap(match => match[1].split(',').map(item => item.trim()));
if (exports.sort().join(',') !== 'GeneralSettingsUI,WorkspaceUI') problems.push('CoreUI.tsx must expose exactly the two retained screens');
if (problems.length) { console.error(problems.join('\n')); process.exitCode = 1; }
else console.log('Core UI boundaries pass: two screens, injected callbacks, no native or network dependencies.');
