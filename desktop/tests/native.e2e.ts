import { $, browser, expect } from '@wdio/globals';
import { mkdtemp, writeFile, readFile, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer, type Socket } from 'node:net';
let directory:string;
const invoke=async(name:string,args:Record<string,unknown>={})=>browser.execute(async(command,parameters)=>{
  return (window as unknown as {__TAURI__:{core:{invoke:(command:string,args:unknown)=>Promise<unknown>}}}).__TAURI__.core.invoke(command,parameters);
},name,args);
describe('Native desktop bridge, no external probes',()=>{
  before(async()=>{directory=await realpath(await mkdtemp(join(tmpdir(),'lantern-desktop-e2e-')));});
  after(async()=>{await rm(directory,{recursive:true,force:true});});
  it('loads the desktop and exposes real runtime capabilities',async()=>{
    await expect($('h1')).toHaveText('Investigate your network.');
    await browser.waitUntil(async()=>!(await $('#environment').getText()).includes('Checking'));
    const info=await invoke('doctor') as {application:string;probes_performed:boolean};expect(info.application).toBe('network-lantern');expect(info.probes_performed).toBe(false);
  });
  it('reviews real Rust path plan and disables a stale preview',async()=>{
    await $('[data-flow="path"]').click();await $('#host').setValue('localhost');await $('#preview').click();
    await expect($('#plan-state')).toHaveText('Plan ready. Review the resolved settings before starting.');
    await expect($('#start')).toBeEnabled();await browser.saveScreenshot(join(tmpdir(),'lantern-native-preview.png'));await $('#host').setValue('other.invalid');await expect($('#start')).toBeDisabled();
  });
  it('cancels an actual run against a bounded loopback listener',async()=>{
    let cookieReceived=false;const sockets=new Set<Socket>();const server=createServer(socket=>{let bytes=0;sockets.add(socket);socket.on('data',chunk=>{bytes+=chunk.length;if(bytes>=37)cookieReceived=true;});socket.on('close',()=>sockets.delete(socket));});
    await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
    try {
      const address=server.address();if(!address||typeof address==='string')throw Error('Loopback listener missing');
      await $('[data-flow="throughput"]').click();await $('#target').setValue('127.0.0.1');await $('#port').setValue(String(address.port));await $('#max-tests').setValue('1');await $('#out').setValue(join(directory,'runs'));
      await $('.advanced summary').click();await $('#advanced').setValue('{"single_test":true,"duration_secs":1,"omit_secs":0,"skip_reachability_check":true,"disable_mtu_probe":true,"retry":{"max_retries":0,"backoff_ms":0}}');
      await $('#preview').click();await expect($('#start')).toBeEnabled();await $('#start').click();await browser.waitUntil(()=>cookieReceived,{timeout:10000,timeoutMsg:'Loopback server did not receive the iperf control cookie'});await expect($('#cancel')).toBeDisplayed();await $('#cancel').click();
      try {
        await browser.waitUntil(async()=>{const state=await invoke('run_status') as {state:string};return state.state==='cancelled';},{timeout:15000,timeoutMsg:'Loopback run did not cancel'});
      } catch(error) {
        throw new Error(`${String(error)}; status=${JSON.stringify(await invoke('run_status'))}; UI=${await $('#run-error').getText()}`);
      }
      await expect($('#start')).toBeDisabled();await expect($('#run-title')).toHaveText('cancelled');
    } finally {for(const socket of sockets)socket.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}
  });
  it('saves, reads and deletes profiles through Rust',async()=>{
    await $('[data-flow="profiles"]').click();await $('#store').setValue(join(directory,'profiles.json'));await $('#profile-name').setValue('Native fixture');await $('#profile-json').setValue('{"target":"localhost","maxTotalTests":0}');
    await $('#profile-form button[type="submit"]').click();await expect($('#profile-list')).toHaveText('Native fixture');
    const data=await invoke('profiles_get',{store:join(directory,'profiles.json'),name:'Native fixture'}) as {target:string};expect(data.target).toBe('localhost');
    await $('#delete-profile').click();await $('#confirm-delete').click();await expect($('#profiles-status')).toHaveText('Deleted profile "Native fixture".');
  });
  it('saves resolved layered configuration without frontend merging',async()=>{
    await $('[data-flow="path"]').click();await $('#host').setValue('localhost');
    if(!(await $('#advanced').isDisplayed()))await $('.advanced summary').click();await $('#advanced').setValue('{"timeout_ms":25,"max_hops":1}');
    await $('#save-config').click();await $('#store').setValue(join(directory,'profiles.json'));await $('#profile-name').setValue('Layered path');await $('#profile-form button[type="submit"]').click();await expect($('#profile-list')).toHaveText('Layered path');
    const saved=await invoke('profiles_get',{store:join(directory,'profiles.json'),name:'Layered path'}) as {capability:string;parameters:{timeout_ms:number;hosts_ipv4:string[]}};
    expect(saved.capability).toBe('path_basic');expect(saved.parameters.timeout_ms).toBe(25);expect(saved.parameters.hosts_ipv4).toEqual(['localhost']);
    await $('#use-profile').click();await $('#preview').click();await expect($('#start')).toBeEnabled();await expect($('#plan-json')).toHaveText(expect.stringContaining('"timeout_ms": 25'));
  });
  it('reads, compares and exports real local reports without invented metrics',async()=>{
    const path=join(directory,'report.json');const destination=join(directory,'export.json');
    await writeFile(path,JSON.stringify({SummaryVersion:2,Timestamp:'2026-01-01T00:00:00Z',Status:'Success',Counts:{Total:25,Failed:0},results:Array.from({length:25},(_,index)=>({fixture_record:index}))}));
    await $('[data-flow="reports"]').click();await $('#report-path').setValue(path);await $('#read-report').click();await expect($('#reports-status')).toHaveText('Report loaded.');
    await expect($('#rows-page')).toHaveText('0–20 of 25');await $('#rows-next').click();await expect($('#rows-page')).toHaveText('20–25 of 25');await expect($('#rows-next')).toBeDisabled();await $('#rows-prev').click();await expect($('#rows-page')).toHaveText('0–20 of 25');
    await browser.saveScreenshot(join(tmpdir(),'lantern-native-report.png'));
    await $('#baseline-path').setValue(path);await $('#compare').click();await expect($('#comparison')).toHaveText(expect.stringContaining('"failed_delta": 0'));
    await $('#export-path').setValue(destination);await $('#export').click();await expect($('#reports-status')).toHaveText('Report exported.');expect(JSON.parse(await readFile(destination,'utf8')).imported.provenance.engine).toBe('legacy');
  });
  it('restores the saved baseline workflow and detailed settings',async()=>{
    await $('[data-flow="baseline"]').click();await $('#host').setValue('localhost');await $('#target').setValue('localhost');
    if(!(await $('#advanced').isDisplayed()))await $('.advanced summary').click();await $('#advanced').setValue('{"throughput":{"duration_secs":1,"omit_secs":0}}');
    await $('#save-config').click();await $('#profile-name').setValue('Baseline snapshot');await $('#profile-form button[type="submit"]').click();await expect($('#profile-list')).toHaveText(expect.stringContaining('Baseline snapshot'));
    const saved=await invoke('profiles_get',{store:join(directory,'profiles.json'),name:'Baseline snapshot'}) as {workflow:string};expect(saved.workflow).toBe('baseline');
    await $('#profile-flow').selectByAttribute('value','triage');await $('#use-profile').click();await expect($('[data-flow="baseline"]')).toHaveAttribute('aria-current','page');await $('#preview').click();await expect($('#start')).toBeEnabled();await expect($('#plan-json')).toHaveText(expect.stringContaining('"single_test": true'));
  });
  it('renders runtime tuning capability status honestly',async()=>{const info=await invoke('doctor') as {capabilities:{tuning:{reason?:string;permission?:string}}};await $('[data-flow="windows_tuning"]').click();await expect($('#start')).toBeDisabled();await expect($('#capability')).toHaveText(info.capabilities.tuning.reason||info.capabilities.tuning.permission||'Native runtime ready.');});
  it('preserves the cancelled report when the native window is closed during a run',async()=>{
    let cookieReceived=false;const sockets=new Set<Socket>();
    const server=createServer(socket=>{let bytes=0;sockets.add(socket);socket.on('data',chunk=>{bytes+=chunk.length;if(bytes>=37)cookieReceived=true;});socket.on('close',()=>sockets.delete(socket));});
    await new Promise<void>(resolve=>server.listen(0,'127.0.0.1',resolve));
    try {
      const address=server.address();if(!address||typeof address==='string')throw Error('Loopback listener missing');
      const out=join(directory,'shutdown');
      const request={capability:'throughput',layers:[{target:'127.0.0.1',port:address.port,single_test:true,protocol:'tcp',duration_secs:1,omit_secs:0,skip_reachability_check:true,disable_mtu_probe:true,retry:{max_retries:0,backoff_ms:0}}]};
      const run=await invoke('start_run',{request,out}) as {run_id:string};
      await browser.waitUntil(()=>cookieReceived,{timeout:10000});
      // A close request goes through the native CloseRequested handler. This
      // permission is confined to tauri.e2e.conf.json and absent in production.
      await browser.execute(()=>{setTimeout(()=>{void (window as unknown as {__TAURI__:{core:{invoke:(name:string,args:unknown)=>Promise<unknown>}}}).__TAURI__.core.invoke('plugin:window|close',{label:'main'});},100);});
      const deadline=Date.now()+15000;let summary:{exit_code:number;status:string}|undefined;
      while(Date.now()<deadline){
        try{summary=JSON.parse(await readFile(join(out,run.run_id,'summary.json'),'utf8'));break;}catch{await new Promise(resolve=>setTimeout(resolve,50));}
      }
      expect(summary?.exit_code).toBe(130);expect(summary?.status).toBe('Cancelled');
      let driverStopped=false;
      while(Date.now()<deadline){
        try{await fetch(`http://127.0.0.1:${browser.options.port}/status`);}catch(error){
          if((error as {cause?:{code?:string}}).cause?.code==='ECONNREFUSED'){driverStopped=true;break;}
          throw error;
        }
        await new Promise(resolve=>setTimeout(resolve,50));
      }
      expect(driverStopped).toBe(true);
      // The embedded driver exits with the application. After independently
      // proving report persistence and driver exit, no live session remains to delete.
      // @wdio/globals exposes a read proxy, so direct assignment would not
      // update the real session. The runtime overwrite API also accepts protocol
      // commands, although its TypeScript key union lists higher-level commands.
      Reflect.apply(browser.overwriteCommand,browser,['deleteSession',async()=>undefined]);
    }finally{for(const socket of sockets)socket.destroy();await new Promise<void>(resolve=>server.close(()=>resolve()));}
  });

});
