import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeEnvironment } from './native-environment.mjs';
const env = nativeEnvironment(process.env);
const result = spawnSync(process.execPath, [
  fileURLToPath(new URL('../node_modules/@wdio/cli/bin/wdio.js', import.meta.url)),
  'run', 'wdio.conf.ts', ...process.argv.slice(2),
], { cwd: new URL('../', import.meta.url), env, stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
