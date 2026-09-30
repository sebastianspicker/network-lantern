import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const config=readFileSync(new URL('../src-tauri/tauri.e2e.conf.json',import.meta.url),'utf8');
const result=spawnSync('cargo',['build','-p','network-lantern-desktop','--features','e2e,tauri/custom-protocol'],{cwd:new URL('../src-tauri/',import.meta.url),env:{...process.env,TAURI_CONFIG:config},stdio:'inherit'});
process.exit(result.status??1);
