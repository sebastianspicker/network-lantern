import { test, expect } from '@playwright/test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
test('seven flows remain usable without a native bridge',async({page})=>{
  const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
  await page.goto('/');await expect(page).toHaveTitle('Network Lantern');await expect(page.getByRole('heading',{level:1})).toBeVisible();
  await expect(page.locator('#environment')).toHaveText('Runtime unavailable');await expect(page.locator('#start')).toBeDisabled();await expect(page.locator('vite-error-overlay')).toHaveCount(0);
  for(const flow of ['path','throughput','baseline','windows_tuning','profiles','reports']){
    await page.locator(`[data-flow="${flow}"]`).click();await expect(page.locator(`[data-flow="${flow}"]`)).toHaveAttribute('aria-current','page');
    await expect(page.locator(`#${['profiles','reports'].includes(flow)?flow:'measurement'}`)).toBeVisible();
  }
  await page.locator('[data-flow="path"]').click();await page.locator('#engine').selectOption('path_trace');await page.locator('#family').selectOption('IPv6');await expect(page.locator('#trace-type')).toHaveValue('ICMP6');await expect(page.locator('#round option')).toHaveCount(8);
  await page.locator('#engine').selectOption('path_basic');await expect(page.locator('#round option')).toHaveCount(3);await expect(page.locator('#trace-type')).toBeDisabled();
  await page.locator('[data-flow=throughput]').focus();await page.keyboard.press('Enter');await expect(page.locator('[data-flow=throughput]')).toHaveAttribute('aria-current','page');await page.locator('[data-flow=path]').focus();await page.keyboard.press('Enter');await expect(page.locator('#host')).toBeVisible();
  await page.locator('#host').fill('localhost');await page.locator('#save-config').click();await expect(page.locator('#profile-json')).toHaveAttribute('readonly','');await expect(page.locator('#profile-json')).toHaveValue(/"capability": "path_basic"/);
  await page.locator('#use-profile').click();await expect(page.locator('#profiles-status')).toContainText('Save this configuration first');expect(errors).toEqual([]);
});
for(const width of [1440,390])test(`responsive workbench at ${width}px`,async({page})=>{
  await page.setViewportSize({width,height:900});await page.goto('/');await expect(page.locator('#environment')).toHaveText('Runtime unavailable');
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await page.screenshot({path:join(tmpdir(),`lantern-desktop-${width}.png`),fullPage:true});
});

// Replace only the browser test's module response; production always invokes Tauri.
async function useNativeFixture(page: import('@playwright/test').Page) {
  await page.route('**/src/bridge.ts', route => route.fulfill({contentType:'text/javascript',body:`
    export const native = true;
    export async function command(name, args = {}) { return window.fixtureCommand(name, args); }
  `}));
  await page.addInitScript(() => {
    const fixture = {state:'running',empty:false,planDelay:0,profiles:{} as Record<string,unknown>,deferred:[] as string[],pending:[] as any[],calls:[] as any[],legacyError:false};
    Object.assign(window,{fixture,fixtureCommand:async(name:string,args:Record<string,any>)=>{
      fixture.calls.push({name,args:JSON.parse(JSON.stringify(args))});
      if(fixture.deferred.includes(name))return new Promise((resolve,reject)=>fixture.pending.push({name,args,resolve,reject}));
      if(name==='doctor')return {os:'fixture',architecture:'test',version:'test',helper:{detail:'Helper ready'},capabilities:Object.fromEntries(['path_basic','path_trace','throughput','tuning'].map(id=>[id,{available:true}]))};
      if(name==='run_status')return fixture.state==='idle'?null:{run_id:'fixture-run',state:fixture.state,completed:2,total:5,logs:['Fixture activity'],exit_code:null,report_path:fixture.state==='running'?null:'fixture.json'};
      if(name==='helper_register'){await new Promise(r=>setTimeout(r,700));throw {category:'permission',message:'Authorization denied'};}
      if(name==='plan'){await new Promise(r=>setTimeout(r,fixture.planDelay));return {capability:args.request.capability,plan:{},total_items:1};}
      if(name==='profiles_list')return Object.keys(fixture.profiles);
      if(name==='profiles_get')return fixture.profiles[args.name] || {};
      if(name==='profiles_save_request'){fixture.profiles[args.name]={capability:args.request.capability,parameters:{host:'saved.example'}};return;}
      if(name==='profiles_save'){fixture.profiles[args.name]=args.parameters;return;}
      if(name==='profiles_delete'){delete fixture.profiles[args.name];return true;}
      if(name==='report_export')return;
      if(name==='runs_list')return {runs:fixture.empty?[]:[{path:'fixture.json',summary:{status:'partial',total:25}}],total:fixture.empty?0:1,has_more:false,legacy_index:fixture.legacyError?{path:'results/runs.json',error:{category:'parse',message:'Invalid legacy index'}}:null};
      if(name==='report_read'){if(args.path==='missing.json')throw {category:'io',message:'Report not found'};return {metadata:{status:'partial',source_schema:'fixture',provenance:{engine:'test'}},rows:fixture.empty?[]:Array.from({length:args.offset===0?20:5},(_,n)=>({record:args.offset+n})),offset:args.offset,next_offset:fixture.empty?0:args.offset===0?20:25,total:fixture.empty?0:25,has_more:!fixture.empty&&args.offset===0};}
      if(name==='report_compare')return {failed_delta:null};
      throw Error('Unexpected fixture command: '+name);
    }});
  });
}

test('helper authorization stays exclusive across polling and denial is actionable',async({page})=>{
  await useNativeFixture(page);await page.goto('/');
  await page.evaluate(()=>{(window as any).fixture.state='idle';});
  await page.locator('.helper-panel summary').click();await expect(page.locator('#helper-register')).toBeEnabled();
  await page.locator('#helper-register').click();await expect(page.locator('#helper-detail')).toContainText('Waiting for');
  await page.waitForTimeout(350); // Cross a status poll while OS authorization is pending.
  await expect(page.locator('#helper-register')).toBeDisabled();await expect(page.locator('#helper-remove')).toBeDisabled();await expect(page.locator('#preview')).toBeDisabled();
  await expect(page.locator('#helper-detail')).toHaveText('permission: Authorization denied');await expect(page.locator('#helper-register')).toBeEnabled();
});

test('tuning plan count and stale asynchronous previews reflect current settings',async({page})=>{
  await useNativeFixture(page);await page.goto('/');await page.evaluate(()=>{(window as any).fixture.state='idle';});
  await page.locator('[data-flow="windows_tuning"]').click();await page.locator('#preview').click();
  await expect(page.locator('#plan-summary')).toContainText('1 planned items');await expect(page.locator('#start')).toBeEnabled();
  await page.locator('#udp-port').fill('5202');await expect(page.locator('#start')).toBeDisabled();await expect(page.locator('#plan-details')).toBeHidden();await expect(page.locator('#plan-summary')).toBeEmpty();
  await page.evaluate(()=>{(window as any).fixture.planDelay=500;});await page.locator('#preview').click();await page.locator('#udp-port').fill('5203');await expect(page.locator('#preview')).toBeEnabled();await expect(page.locator('#start')).toBeDisabled();await expect(page.locator('#plan-details')).toBeHidden();
});

test('invalid profiles remain editable and tuning snapshots choose their workflow',async({page})=>{
  await useNativeFixture(page);await page.goto('/');await page.locator('[data-flow="profiles"]').click();
  await page.locator('#profile-json').fill('[]');await page.locator('#use-profile').click();await expect(page.locator('#profiles-status')).toHaveText('Profile parameters must be a JSON object.');await expect(page.locator('#profiles')).toBeVisible();
  await page.locator('#profile-json').fill('{"capability":"tuning","parameters":{"action":"Verify"}}');await page.locator('#use-profile').click();await expect(page.locator('[data-flow="windows_tuning"]')).toHaveAttribute('aria-current','page');await expect(page.locator('#start')).toBeDisabled();
});

test('run terminal states preserve partial counts and expose recorded evidence',async({page})=>{
  await useNativeFixture(page);await page.goto('/');await expect(page.locator('#cancel')).toBeEnabled();
  for(const state of ['cancelling','cancelled','failed','partial_failure']){
    await page.evaluate(state=>{(window as any).fixture.state=state;},state);await expect(page.locator('#run-title')).toHaveText(state.replaceAll('_',' '));await expect(page.locator('#run-count')).toContainText('2 of 5');
    if(state==='cancelling')await expect(page.locator('#cancel')).toBeDisabled();else{await expect(page.locator('#cancel')).toBeHidden();await expect(page.locator('#open-run')).toBeVisible();}
  }
});

test('reports page through evidence, clear stale comparisons and show empty/error states',async({page})=>{
  await useNativeFixture(page);await page.goto('/');await page.locator('[data-flow="reports"]').click();await expect(page.locator('#run-list')).toContainText('partial');
  await page.locator('[data-report]').click();await expect(page.locator('#rows-page')).toHaveText('0–20 of 25');await page.locator('#rows-next').click();await expect(page.locator('#rows-page')).toHaveText('20–25 of 25');await expect(page.locator('#rows-next')).toBeDisabled();await page.locator('#rows-prev').click();await expect(page.locator('#rows-page')).toHaveText('0–20 of 25');
  await page.locator('#compare').click();await expect(page.locator('#comparison')).toBeVisible();await page.locator('#report-path').fill('missing.json');await page.locator('#read-report').click();await expect(page.locator('#reports-status')).toHaveText('io: Report not found');await expect(page.locator('#report-detail')).toBeHidden();await expect(page.locator('#comparison')).toBeHidden();
  await page.evaluate(()=>{(window as any).fixture.empty=true;});await page.locator('#refresh-reports').click();await expect(page.locator('#reports-status')).toHaveText('No recorded runs in this directory.');await expect(page.locator('#runs-next')).toBeDisabled();await expect(page.locator('#runs-prev')).toBeDisabled();
  await page.locator('#report-path').fill('empty.json');await page.locator('#read-report').click();await expect(page.locator('#rows-page')).toHaveText('0–0 of 0');await expect(page.locator('#rows-next')).toBeDisabled();await expect(page.locator('#rows-prev')).toBeDisabled();
  await page.screenshot({path:join(tmpdir(),'lantern-desktop-reports-empty.png'),fullPage:true});
});

async function deferCommands(page: import('@playwright/test').Page, ...names: string[]) {
  await page.evaluate(names => { (window as any).fixture.deferred = names; }, names);
}
async function settleCommand(page: import('@playwright/test').Page, name: string, args: Record<string, unknown>, result: unknown, error = false) {
  await expect.poll(() => page.evaluate(({name,args}) => (window as any).fixture.pending.some((item: any) => item.name === name && Object.entries(args).every(([key,value]) => item.args[key] === value)), {name,args})).toBe(true);
  await page.evaluate(({name,args,result,error}) => {
    const pending = (window as any).fixture.pending;
    const index = pending.findIndex((item: any) => item.name === name && Object.entries(args).every(([key,value]) => item.args[key] === value));
    const [item] = pending.splice(index, 1);
    if (error) item.reject(result); else item.resolve(result);
  }, {name,args,result,error});
}
async function openProfilesFixture(page: import('@playwright/test').Page) {
  await useNativeFixture(page);
  await page.goto('/');
  await page.evaluate(() => {
    (window as any).fixture.profiles = {Alpha:{host:'alpha.example'},Beta:{host:'beta.example'}};
  });
  await page.locator('[data-flow="profiles"]').click();
  await expect(page.locator('#profile-list')).toContainText('Beta');
}
function reportPage(label: string) {
  return {metadata:{status:label,source_schema:'fixture',provenance:{}},rows:[{label}],offset:0,next_offset:1,total:1,has_more:false};
}

for (const staleError of [false,true]) test(`profile selection ignores stale ${staleError ? 'errors' : 'results'}`, async ({page}) => {
  await openProfilesFixture(page);
  await deferCommands(page, 'profiles_get');
  await page.locator('[data-profile="Alpha"]').click();
  await page.locator('[data-profile="Beta"]').click();
  await settleCommand(page, 'profiles_get', {name:'Beta'}, {host:'beta.example'});
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await settleCommand(page, 'profiles_get', {name:'Alpha'}, staleError ? {category:'io',message:'Stale profile failure'} : {host:'alpha.example'}, staleError);
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await expect(page.locator('#profile-json')).toHaveValue(/beta.example/);
  await expect(page.locator('#profiles-status')).toHaveText('Profile loaded for review.');
});

test('failed or edited pending profile reads preserve the complete previous draft', async ({page}) => {
  await openProfilesFixture(page);
  await page.locator('[data-profile="Beta"]').click();
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await deferCommands(page, 'profiles_get');
  await page.locator('[data-profile="Alpha"]').click();
  await expect(page.locator('#profile-form button[type="submit"]')).toBeDisabled();
  await settleCommand(page, 'profiles_get', {name:'Alpha'}, {category:'io',message:'Profile unavailable'}, true);
  await expect(page.locator('#profiles-status')).toHaveText('io: Profile unavailable');
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await expect(page.locator('#profile-json')).toHaveValue(/beta.example/);
  await page.locator('[data-profile="Alpha"]').click();
  await page.locator('#profile-flow').selectOption('path');
  await settleCommand(page, 'profiles_get', {name:'Alpha'}, {host:'alpha.example'});
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await expect(page.locator('#profile-json')).toHaveValue(/beta.example/);
  await page.locator('#profile-form button[type="submit"]').click();
  await expect(page.locator('#profiles-status')).toHaveText('Saved profile "Beta".');
  const saved = await page.evaluate(() => (window as any).fixture.calls.findLast((item:any) => item.name === 'profiles_save'));
  expect(saved.args).toMatchObject({name:'Beta',parameters:{host:'beta.example'}});
});

for (const staleError of [false,true]) test(`snapshot save readback ignores stale ${staleError ? 'errors' : 'results'} after selecting another profile`, async ({page}) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text()); });
  await useNativeFixture(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).fixture.state = 'idle'; (window as any).fixture.profiles = {Beta:{host:'beta.example'}}; });
  await page.locator('[data-flow="path"]').click();
  await page.locator('#host').fill('snapshot.example');
  await page.locator('#save-config').click();
  await page.locator('#profile-name').fill('Alpha');
  await deferCommands(page, 'profiles_get');
  await page.locator('#profile-form button[type="submit"]').click();
  await expect(page.locator('#profile-form button[type="submit"]')).toBeDisabled();
  await page.locator('[data-profile="Beta"]').click();
  await settleCommand(page, 'profiles_get', {name:'Beta'}, {host:'beta.example'});
  await settleCommand(page, 'profiles_get', {name:'Alpha'}, staleError ? {category:'io',message:'Stale readback failure'} : {host:'snapshot.example'}, staleError);
  await expect(page.locator('#profile-form button[type="submit"]')).toBeEnabled();
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await expect(page.locator('#profile-json')).toHaveValue(/beta.example/);
  await expect(page.locator('#profiles-status')).toHaveText('Profile loaded for review.');
  await expect(page.locator('vite-error-overlay')).toHaveCount(0);
  await expect(page).toHaveTitle('Network Lantern');
  await expect(page.getByRole('heading',{level:1})).toBeVisible();
  expect(errors).toEqual([]);
  if (!staleError) {
    await expect(page.locator('#run-strip')).toBeHidden();
    await page.screenshot({path:join(tmpdir(),'lantern-desktop-profile-race.png'),fullPage:true});
  }
});

test('profile writes serialize captured inputs and deletion confirmation follows its target', async ({page}) => {
  await openProfilesFixture(page);
  await page.locator('[data-profile="Alpha"]').click();
  await expect(page.locator('#profile-name')).toHaveValue('Alpha');
  await deferCommands(page, 'profiles_save');
  await page.locator('#profile-form button[type="submit"]').click();
  await expect(page.locator('#delete-profile')).toBeDisabled();
  await page.locator('#profile-name').fill('Edited draft');
  await page.locator('#profile-json').fill('{"host":"edited.example"}');
  await page.locator('#profile-form').evaluate(form => form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true})));
  const saves = await page.evaluate(() => (window as any).fixture.calls.filter((item:any) => item.name === 'profiles_save'));
  expect(saves).toHaveLength(1);
  expect(saves[0].args).toMatchObject({name:'Alpha',parameters:{host:'alpha.example'}});
  await settleCommand(page, 'profiles_save', {name:'Alpha'}, null);
  await expect(page.locator('#profile-form button[type="submit"]')).toBeEnabled();
  await expect(page.locator('#profile-name')).toHaveValue('Edited draft');
  await expect(page.locator('#profile-json')).toHaveValue('{"host":"edited.example"}');
  await deferCommands(page);
  await page.locator('[data-profile="Alpha"]').click();
  await expect(page.locator('#profile-name')).toHaveValue('Alpha');
  await page.locator('#delete-profile').click();
  await expect(page.locator('#delete-confirm')).toBeVisible();
  await page.locator('[data-profile="Beta"]').click();
  await expect(page.locator('#delete-confirm')).toBeHidden();
  await expect(page.locator('#profile-name')).toHaveValue('Beta');
  await page.locator('#delete-profile').click();
  await page.locator('#confirm-delete').click();
  await expect(page.locator('#profiles-status')).toHaveText('Deleted profile "Beta".');
  const deletions = await page.evaluate(() => (window as any).fixture.calls.filter((item:any) => item.name === 'profiles_delete'));
  expect(deletions).toHaveLength(1);
  expect(deletions[0].args.name).toBe('Beta');
  await expect(page.locator('#profile-list')).not.toContainText('Beta');
});

for (const staleError of [false,true]) test(`library lists ignore stale ${staleError ? 'errors' : 'results'} after changing stores and directories`, async ({page}) => {
  await openProfilesFixture(page);
  await deferCommands(page, 'profiles_list');
  await page.locator('#load-profiles').click();
  await page.locator('#store').fill('other-profiles.json');
  await page.locator('#load-profiles').click();
  await settleCommand(page, 'profiles_list', {store:'other-profiles.json'}, ['Current profile']);
  await settleCommand(page, 'profiles_list', {store:'.iperf3/profiles.json'}, staleError ? {category:'io',message:'Stale store failure'} : ['Stale profile'], staleError);
  await expect(page.locator('#profile-list')).toHaveText('Current profile');
  await expect(page.locator('#profiles-status')).toHaveText('1 saved profiles');
  await deferCommands(page, 'runs_list');
  await page.locator('[data-flow="reports"]').click();
  await page.locator('#report-directory').fill('other-results');
  await page.locator('#refresh-reports').click();
  await settleCommand(page, 'runs_list', {directory:'other-results'}, {runs:[{path:'current.json',summary:{status:'current',total:1}}],total:1,has_more:false});
  await settleCommand(page, 'runs_list', {directory:'results'}, staleError ? {category:'io',message:'Stale directory failure'} : {runs:[{path:'stale.json',summary:{status:'stale'}}],total:1,has_more:false}, staleError);
  await expect(page.locator('#run-list')).toContainText('current');
  await expect(page.locator('#run-list')).not.toContainText('stale');
  await expect(page.locator('#reports-status')).toHaveText('Runs loaded.');
});

for (const staleError of [false,true]) test(`report reads and comparisons ignore stale ${staleError ? 'errors' : 'results'} after opening another report`, async ({page}) => {
  await useNativeFixture(page);
  await page.goto('/');
  await page.locator('[data-flow="reports"]').click();
  await deferCommands(page, 'report_read');
  await page.locator('#report-path').fill('old.json');
  await page.locator('#read-report').click();
  await page.locator('#report-path').fill('current.json');
  await page.locator('#read-report').click();
  await settleCommand(page, 'report_read', {path:'current.json'}, reportPage('current'));
  await settleCommand(page, 'report_read', {path:'old.json'}, staleError ? {category:'io',message:'Stale report failure'} : reportPage('stale'), staleError);
  await expect(page.locator('#report-metadata')).toContainText('current.json');
  await expect(page.locator('#report-rows')).toContainText('current');
  await expect(page.locator('#reports-status')).toHaveText('Report loaded.');
  await deferCommands(page, 'report_compare');
  await page.locator('#baseline-path').fill('baseline.json');
  await page.locator('#compare').click();
  await page.locator('#report-path').fill('new.json');
  await page.locator('#read-report').click();
  await expect(page.locator('#report-metadata')).toContainText('new.json');
  await page.locator('#compare').click();
  await settleCommand(page, 'report_compare', {current:'new.json'}, {label:'new comparison'});
  await settleCommand(page, 'report_compare', {current:'current.json'}, staleError ? {category:'io',message:'Stale comparison failure'} : {label:'stale comparison'}, staleError);
  await expect(page.locator('#comparison')).toHaveText(/new comparison/);
  await expect(page.locator('#reports-status')).toHaveText('Comparison ready. Null values indicate unavailable evidence.');
});

test('baseline edits invalidate pending comparisons and exports serialize captured destinations', async ({page}) => {
  await useNativeFixture(page);
  await page.goto('/');
  await page.locator('[data-flow="reports"]').click();
  await page.locator('[data-report]').click();
  await deferCommands(page, 'report_compare','report_export');
  await page.locator('#baseline-path').fill('old-baseline.json');
  await page.locator('#compare').click();
  await page.locator('#baseline-path').fill('new-baseline.json');
  await settleCommand(page, 'report_compare', {baseline:'old-baseline.json'}, {label:'stale baseline'});
  await expect(page.locator('#comparison')).toBeHidden();
  await expect(page.locator('#reports-status')).toBeEmpty();
  await page.locator('#export-path').fill('old-export.json');
  await page.locator('#export').click();
  await expect(page.locator('#export')).toBeDisabled();
  await page.locator('#export-path').fill('new-export.json');
  await page.locator('#export').evaluate(button => button.dispatchEvent(new Event('click')));
  const exports = await page.evaluate(() => (window as any).fixture.calls.filter((item:any) => item.name === 'report_export'));
  expect(exports).toHaveLength(1);
  expect(exports[0].args).toMatchObject({path:'fixture.json',destination:'old-export.json'});
  await settleCommand(page, 'report_export', {destination:'old-export.json'}, {category:'io',message:'Stale export failure'}, true);
  await expect(page.locator('#export')).toBeEnabled();
  await expect(page.locator('#reports-status')).toBeEmpty();
});

test('unreadable legacy indexes warn while valid native runs remain available', async ({page}) => {
  await useNativeFixture(page);
  await page.goto('/');
  await page.evaluate(() => { (window as any).fixture.legacyError = true; });
  await page.locator('[data-flow="reports"]').click();
  await expect(page.locator('#run-list')).toContainText('partial');
  await expect(page.locator('#reports-status')).toHaveText('Runs loaded. Legacy index results/runs.json: parse: Invalid legacy index');
  await page.locator('[data-report]').click();
  await expect(page.locator('#report-detail')).toBeVisible();
});
