import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
export const config = {
  runner:'local',outputDir:join(tmpdir(),'lantern-native-wdio'),specs:['./tests/native.e2e.ts'],maxInstances:1,
  capabilities:[{browserName:'tauri'}],
  services:[['tauri',{appBinaryPath:fileURLToPath(new URL(`../target/debug/network-lantern-desktop${process.platform==='win32'?'.exe':''}`,import.meta.url)),driverProvider:'embedded',captureBackendLogs:true}]],
  afterTest:(_test:unknown,_context:unknown,{error}:{error?:Error})=>{if(error)console.error(error.stack||error.message);},
  framework:'mocha',reporters:['spec'],mochaOpts:{timeout:60000},logLevel:'warn',waitforTimeout:15000,
};
