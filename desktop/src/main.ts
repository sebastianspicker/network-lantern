import './style.css';
import { command, native } from './bridge';
import { active, errorMessage, escape as e, fingerprint, requestFor, type Doctor, type Flow, type Inputs, type Json, type Progress, type Request } from './model';
if (import.meta.env.MODE === 'e2e') await import('@wdio/tauri-plugin');
const flows: [Flow, string, string][] = [['triage','Triage','Path and throughput together'],['path','Path','Reachability and routing'],['throughput','Throughput','Measure a test matrix'],['baseline','Baseline','Path and one sample'],['windows_tuning','Windows tuning','Review and recover settings'],['profiles','Profiles','Reusable configurations'],['reports','Reports','Browse and compare evidence']];
const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const field = (id: string, label: string, value = '', type = 'text', attrs = '') => `<label class="field" for="${id}"><span>${label}</span><input id="${id}" type="${type}" value="${value}" ${attrs}></label>`;
const select = (id: string, label: string, values: string[]) => `<label class="field" for="${id}"><span>${label}</span><select id="${id}">${values.map(v=>`<option>${v}</option>`).join('')}</select></label>`;
$('app').innerHTML = `<a class="skip-link" href="#content">Skip to workspace</a><header><div class="brand"><svg viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="16" r="15"/><path d="M11 10h10l2 16H9l2-16Zm2 0V7a3 3 0 0 1 6 0v3M12 13h8M16 15c-4 4-3 7 0 7s4-3 0-7Z"/></svg>Network Lantern</div><span id="environment">Checking runtime…</span><button id="refresh-runtime" class="secondary">Check runtime</button></header><main><div class="intro"><h1>Investigate your network.</h1><p>Configure a workflow. Review its plan. Keep the evidence.</p></div><div id="runtime-notice" class="notice" role="status"></div><details class="helper-panel"><summary>Privileged helper</summary><p>Register the helper when native probes or Windows changes require authorization. The operating system controls approval.</p><p id="helper-detail" role="status">Checking helper…</p><div class="actions"><button id="helper-register" class="secondary">Register helper</button><button id="helper-remove" class="danger">Remove helper</button></div></details><section id="run-strip" class="run-strip" aria-label="Active measurement" hidden><div><strong id="run-title" role="status"></strong><p id="run-count"></p></div><progress id="run-progress"></progress><button id="cancel" class="danger">Cancel run</button><button id="open-run" class="secondary" hidden>Open report</button><details><summary>Recent activity</summary><pre id="run-logs" tabindex="0"></pre></details><p id="run-error" class="error" role="alert"></p></section><div class="workbench"><nav aria-label="Workflows">${flows.map(([id,label,desc])=>`<button data-flow="${id}" aria-current="${id==='triage'?'page':'false'}"><strong>${label}</strong><span>${desc}</span></button>`).join('')}</nav><section id="content" tabindex="-1"><div id="measurement"><div class="controls"><h2 id="flow-title">Triage</h2><p id="flow-description" class="muted">Path and throughput together</p><form id="configuration"><fieldset id="path-fields"><legend>Path diagnostics</legend>${field('host','Host','', 'text','placeholder="host.example.net" autocomplete="off" spellcheck="false" maxlength="253"')}<div class="two">${select('family','Address family',['IPv4','IPv6'])}${select('round','Round',['Standard','MTU1400_DF','TTL64_Timeout5s'])}</div><div id="engine-fields">${select('engine','Engine',['path_basic','path_trace'])}${select('trace-type','Trace mode',['ICMP4','TCP4','UDP4','MPLS4','AS4'])}<p class="hint">AS4/AS6 discloses hop addresses to Team Cymru DNS.</p></div><label class="check"><input id="skip" type="checkbox">Skip pathping</label></fieldset><fieldset id="throughput-fields"><legend>Throughput</legend>${field('target','Server','', 'text','placeholder="iperf.example.net" autocomplete="off" spellcheck="false" maxlength="253"')}<div class="two">${field('port','Port','5201','number','min="1" max="65535" required')}${select('protocol','Protocol',['Both','TCP','UDP'])}</div>${field('max-tests','Maximum tests','0','number','min="0" max="1000000" required')}<p class="hint">0 means unlimited. The Rust preview checks the full matrix against this limit.</p></fieldset><fieldset id="tuning-fields" hidden><legend>Windows tuning</legend><div class="two">${select('action','Action',['Verify','Backup','Apply','Restore'])}${select('tuning-profile','Profile',['Safe','Measured'])}</div>${field('udp-port','Managed UDP port','5201','number','min="1" max="65535" required')}${field('dscp','DSCP','46','number','min="0" max="63" required')}${field('backup-folder','Backup / restore folder','','text','placeholder="Default protected backup location"')}${select('power-plan','Power plan',['None','HighPerformance'])}<label class="check"><input id="app-policies" type="checkbox">Manage application QoS policies</label><label class="field" for="app-paths"><span>Application paths, one per line</span><textarea id="app-paths" rows="3" spellcheck="false"></textarea></label><p class="hint">Review platform support and recovery requirements before any change.</p></fieldset><details class="advanced"><summary>Additional settings</summary><label class="field" for="advanced"><span>JSON override layer</span><textarea id="advanced" rows="6" spellcheck="false">{}</textarea></label><p class="hint">Rust validates keys and values. Use this layer for detailed engine settings or loaded profiles.</p><label class="check"><input id="strict" type="checkbox">Reject unknown settings</label></details>${field('out','Results directory','results','text','required')}<div class="actions"><button id="preview" type="submit">Review plan</button><button id="save-config" type="button" class="secondary">Save as profile</button></div></form></div><section class="preview" aria-labelledby="preview-title"><h2 id="preview-title">Your preview</h2><p class="muted">Review targets, scope, and permissions before starting.</p><div id="capability" class="notice"></div><p id="plan-state" role="status">Configure your workflow and review its plan.</p><div id="plan-error" class="error" role="alert"></div><div id="plan-summary"></div><details id="plan-details" hidden><summary>Resolved plan</summary><pre id="plan-json" tabindex="0"></pre></details><div class="start-area"><button id="start" disabled>Start reviewed run</button><p class="hint">Starting sends network traffic or performs the selected tuning action.</p></div></section></div><section id="profiles" class="library" hidden><h2>Profiles</h2><p class="muted">Store reusable settings locally. Loaded profiles still require a fresh plan.</p>${field('store','Profile store','.iperf3/profiles.json')}<button id="load-profiles" class="secondary">Refresh profiles</button><p id="profiles-status" role="status"></p><div class="library-grid"><div><h3>Saved profiles</h3><ul id="profile-list" class="item-list"></ul></div><form id="profile-form">${field('profile-name','Name','','text','required maxlength="128"')}${select('profile-flow','Load into workflow',['triage','path','throughput','baseline','windows_tuning'])}<label class="field" for="profile-json"><span id="profile-json-label">Parameters (JSON object)</span><textarea id="profile-json" rows="12" spellcheck="false">{}</textarea></label><div class="actions"><button type="submit">Save profile</button><button id="use-profile" type="button" class="secondary">Load into workflow</button><button id="delete-profile" type="button" class="danger">Delete</button></div><p id="delete-confirm" hidden>Delete this saved profile? <button type="button" id="confirm-delete" class="danger">Confirm deletion</button><button type="button" id="keep-profile" class="secondary">Keep profile</button></p></form></div></section><section id="reports" class="library" hidden><h2>Reports</h2><p class="muted">Inspect recorded results and compare runs. Missing metrics remain unavailable.</p><div class="two">${field('report-directory','Results directory','results')}${field('report-path','Report path','','text','placeholder="results/run-id/summary.json"')}</div><div class="actions"><button id="refresh-reports" class="secondary">Refresh runs</button><button id="read-report" class="secondary">Open report</button></div><p id="reports-status" role="status"></p><div class="table-scroll"><table><thead><tr><th>Run</th><th>Status</th><th>Measurements</th><th></th></tr></thead><tbody id="run-list"></tbody></table></div><div class="actions"><button id="runs-prev" class="secondary">Previous runs</button><button id="runs-next" class="secondary">Next runs</button><span id="runs-page"></span></div><section id="report-detail" hidden><h3>Selected report</h3><div id="report-metadata"></div><div class="actions"><button id="rows-prev" class="secondary">Previous measurements</button><button id="rows-next" class="secondary">Next measurements</button><span id="rows-page"></span></div><pre id="report-rows" tabindex="0"></pre><div class="two">${field('baseline-path','Baseline report path')}${field('export-path','Export destination','','text','placeholder="exports/review.json"')}</div><div class="actions"><button id="compare" class="secondary">Compare with baseline</button><button id="export" class="secondary">Export JSON</button></div><pre id="comparison" tabindex="0" hidden></pre></section></section></section></div><footer>Local Rust runtime · One active measurement · Review before execution</footer></main>`;
for (const option of ($('engine') as HTMLSelectElement).options) { const key=option.value; option.value=key; option.textContent=key==='path_basic'?'Basic path diagnostics':'Continuous path trace'; }
for (const option of ($('profile-flow') as HTMLSelectElement).options) { const key=option.value; option.value=key; option.textContent=flows.find(([id])=>id===key)?.[1]||key; }
let flow: Flow = 'triage'; let previousFlow: Flow = 'triage'; let doctor: Doctor | null = null; let run: Progress | null = null; let previewKey = ''; let previewRequest: Request | null = null; let pendingProfileRequest: Request | null = null; let generation = 0; let starting = false; let planning = false; let helperBusy = false;
const value = (id: string) => $(id) as HTMLInputElement;
let loadedWorkflowEnvelope=false;
const inputs = (): Inputs => ({host:value('host').value.trim(),family:value('family').value,round:value('round').value,engine:value('engine').value,traceType:value('trace-type').value,skip:value('skip').checked,target:value('target').value,port:Number(value('port').value),protocol:value('protocol').value,maxTests:Number(value('max-tests').value),action:value('action').value,profile:value('tuning-profile').value,udpPort:Number(value('udp-port').value),advanced:value('advanced').value,strict:value('strict').checked,backupFolder:value('backup-folder').value,dscp:Number(value('dscp').value),powerPlan:value('power-plan').value,includeAppPolicies:value('app-policies').checked,appPaths:value('app-paths').value.split('\n').map(v=>v.trim()).filter(Boolean)});
function currentRequest() { const request=requestFor(flow, inputs()); if(loadedWorkflowEnvelope && ['path','throughput','windows_tuning'].includes(flow)){request.capability='workflow';request.workflow=flow;request.layers=[JSON.parse(value('advanced').value) as Json];} return request; }
function invalidate() { $('plan-summary').replaceChildren(); $('plan-details').hidden=true; generation++; previewKey=''; previewRequest=null; $('start').setAttribute('disabled',''); $('plan-state').textContent='Settings changed. Review a fresh plan before starting.'; }
function capability() {
  if (!doctor) return { available:false, reason:'Native runtime unavailable. Open the desktop application.' };
  const id=flow==='throughput'?'throughput':flow==='windows_tuning'?'tuning':flow==='path'?value('engine').value:'path_basic';
  return doctor.capabilities[id] || { available:false, reason:'Capability information unavailable.' };
}
function updateAvailability() {
  const cap=capability(); $('capability').textContent=cap.reason || cap.permission || 'Native runtime ready.';
  let fresh=false; try { fresh=previewKey===fingerprint(currentRequest(),value('out').value) && !!previewKey; } catch { /* Invalid JSON awaits Rust review. */ }
  value('start').disabled=!native||!cap.available||!fresh||active(run)||starting||planning||helperBusy;
  value('preview').disabled=!native||planning||starting||helperBusy;
  value('helper-register').disabled=!native||active(run)||starting||helperBusy;
  value('helper-remove').disabled=!native||active(run)||starting||helperBusy;
}
function navigate(next: Flow) {
  invalidateLibraryWork();
  if (!['profiles','reports'].includes(flow)) previousFlow=flow;
  flow=next; loadedWorkflowEnvelope=false; invalidate(); document.querySelectorAll<HTMLButtonElement>('[data-flow]').forEach(b=>b.setAttribute('aria-current',b.dataset.flow===flow?'page':'false'));
  $('measurement').hidden=['profiles','reports'].includes(flow); $('profiles').hidden=flow!=='profiles'; $('reports').hidden=flow!=='reports';
  const entry=flows.find(([id])=>id===flow)!; $('flow-title').textContent=entry[1]; $('flow-description').textContent=entry[2];
  for (const [id, show] of [['path-fields',['triage','path','baseline'].includes(flow)],['throughput-fields',['triage','throughput','baseline'].includes(flow)],['tuning-fields',flow==='windows_tuning']] as const) { $(id).hidden=!show; ($(id) as HTMLFieldSetElement).disabled=!show; }
  $('engine-fields').hidden=flow!=='path'; syncPathOptions(); value('target').required=false;
  $('plan-summary').replaceChildren(); $('plan-details').hidden=true; $('plan-error').textContent=''; updateAvailability();
  if(flow==='profiles') void loadProfiles(); if(flow==='reports') void loadRuns(0);
}
function syncPathOptions() {
  const trace=flow==='path' && value('engine').value==='path_trace';
  const rounds=trace?['Standard','MTU1400','TOS_CS5','TOS_AF11','TTL10','TTL64','FirstTTL3','Timeout5']:['Standard','MTU1400_DF','TTL64_Timeout5s'];
  const prior=value('round').value; $('round').innerHTML=rounds.map(v=>`<option>${v}</option>`).join(''); value('round').value=rounds.includes(prior)?prior:'Standard';
  const suffix=value('family').value==='IPv4'?'4':'6'; const priorType=value('trace-type').value.replace(/[46]$/,'');
  $('trace-type').innerHTML=['ICMP','TCP','UDP','MPLS','AS'].map(v=>`<option>${v}${suffix}</option>`).join(''); value('trace-type').value=priorType+suffix;
  value('trace-type').disabled=!trace;
}
$('engine').addEventListener('change',syncPathOptions); $('family').addEventListener('change',syncPathOptions);
document.querySelectorAll<HTMLButtonElement>('[data-flow]').forEach(b=>b.addEventListener('click',()=>navigate(b.dataset.flow as Flow)));
$('configuration').addEventListener('input',()=>{invalidate(); updateAvailability();}); $('configuration').addEventListener('change',()=>{invalidate(); updateAvailability();});
$('configuration').addEventListener('submit',async event=>{
  event.preventDefault(); const token=++generation; planning=true; $('plan-error').textContent=''; $('plan-state').textContent='Resolving and validating the plan…'; updateAvailability();
  try { const request=currentRequest(); const key=fingerprint(request,value('out').value); const result=await command<Json>('plan',{request}); if(token!==generation)return; previewRequest=(result.resolved_request as unknown as Request|undefined)??request; previewKey=key; $('plan-state').textContent='Plan ready. Review the resolved settings before starting.'; $('plan-json').textContent=JSON.stringify(result,null,2); $('plan-details').hidden=false; ($('plan-details') as HTMLDetailsElement).open=true;
    const steps=(result.steps as Json[]|undefined)||[result]; $('plan-summary').innerHTML=`<ol class="steps">${steps.map(s=>`<li><strong>${e(s.capability)}</strong><p>${e((s.plan as Json|undefined)?.total_tests ?? (s.plan as Json|undefined)?.total_items ?? s.total_items ?? 'See resolved plan')} planned items</p></li>`).join('')}</ol>${(result.warnings as string[]||[]).map(w=>`<p class="notice">${e(w)}</p>`).join('')}`;
  } catch(error) { if(token===generation) {previewKey=''; previewRequest=null; $('plan-error').textContent=errorMessage(error); $('plan-state').textContent='Plan unavailable. Resolve the issue and review again.';} }
  finally {planning=false;updateAvailability();}
});
$('start').addEventListener('click',async()=>{
  if(!previewRequest||!previewKey||previewKey!==fingerprint(currentRequest(),value('out').value)||active(run)||starting||helperBusy)return;
  starting=true; updateAvailability(); $('plan-error').textContent='';
  try {await command('start_run',{request:previewRequest,out:value('out').value}); invalidate(); await pollRun();} catch(error) {$('plan-error').textContent=errorMessage(error);} finally {starting=false; updateAvailability();}
});
$('cancel').addEventListener('click',async()=>{if(!run)return; value('cancel').disabled=true; try {$('run-error').textContent=''; await command('cancel_run',{runId:run.run_id}); await pollRun();}catch(error){$('run-error').textContent=errorMessage(error); value('cancel').disabled=false;}});
let polling=false;
async function pollRun() { if(!native||polling)return; polling=true; try {run=await command<Progress|null>('run_status'); renderRun();}catch(error){$('run-error').textContent=errorMessage(error);}finally{polling=false;updateAvailability();} }
function renderRun(){ $('run-strip').hidden=!run;if(!run)return; const stateLabel=run.state.replaceAll('_',' '); if($('run-title').textContent!==stateLabel)$('run-title').textContent=stateLabel; $('run-count').textContent=`${run.completed} of ${run.total} measurements · ${run.run_id}`; const progress=$<HTMLProgressElement>('run-progress');progress.max=Math.max(run.total,1);progress.value=run.completed; $('run-logs').textContent=run.logs.slice(-64).join('\n')||'No activity recorded yet.'; value('cancel').disabled=run.state!=='running'; $('cancel').hidden=!active(run); $('open-run').hidden=!run.report_path; }
$('open-run').addEventListener('click',()=>{if(run?.report_path){navigate('reports'); value('report-path').value=run.report_path;void readReport(0);}});
async function checkRuntime(){ $('environment').textContent='Checking runtime…'; try {doctor=await command<Doctor>('doctor');$('environment').textContent=`${doctor.os} · ${doctor.architecture} · ${doctor.version}`; $('runtime-notice').textContent='Local runtime ready. Review capability permissions before starting.'; $('helper-detail').textContent=String(doctor.helper.detail||doctor.helper.registration||(doctor.helper.error?errorMessage(doctor.helper.error):'Helper status unavailable')); }catch(error){doctor=null;$('environment').textContent='Runtime unavailable';$('helper-detail').textContent='Helper status unavailable while the native runtime is disconnected.';$('runtime-notice').textContent=errorMessage(error);}updateAvailability(); }
$('refresh-runtime').addEventListener('click',()=>void checkRuntime());
for(const operation of ['register','remove']) $('helper-'+operation).addEventListener('click',async()=>{if(helperBusy||active(run)||starting)return;helperBusy=true;updateAvailability();$('helper-detail').textContent=operation==='register'?'Waiting for helper registration authorization…':'Removing helper…';try{const status=await command<Json>('helper_'+operation);$('helper-detail').textContent=String(status.detail||status.registration);await checkRuntime();}catch(error){$('helper-detail').textContent=errorMessage(error);}finally{helperBusy=false;updateAvailability();}});
type LibraryStatus = 'profiles-status' | 'reports-status';
let libraryRevision = 0;
const statusRevisions: Record<LibraryStatus, number> = {'profiles-status':0, 'reports-status':0};
function beginTask(status: LibraryStatus, isCurrent: () => boolean) {
  const revision = ++statusRevisions[status];
  $(status).textContent = 'Working…';
  return {
    current: isCurrent,
    message(text: string) {
      if (isCurrent() && revision === statusRevisions[status]) $(status).textContent = text;
    },
    async run(operation: () => Promise<void>) {
      try { await operation(); }
      catch (error) { this.message(errorMessage(error)); }
    }
  };
}
function invalidateStatus(status: LibraryStatus) {
  statusRevisions[status]++;
  $(status).textContent = '';
}
let profilesGeneration = 0;
let profileRevision = 0;
let profileLoading = false;
let profileMutationBusy = false;
let deleteTarget: {store:string;name:string;revision:number} | null = null;
function updateProfileControls() {
  $<HTMLFormElement>('profile-form').querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled = profileMutationBusy || profileLoading;
  value('delete-profile').disabled = profileMutationBusy || profileLoading;
  value('confirm-delete').disabled = profileMutationBusy || profileLoading;
  value('use-profile').disabled = profileLoading;
}
function invalidateProfileEditor() {
  profileRevision++;
  profileLoading = false;
  deleteTarget = null;
  $('delete-confirm').hidden = true;
  updateProfileControls();
}
function showProfileParameters(data: Json) {
  pendingProfileRequest = null;
  $<HTMLTextAreaElement>('profile-json').readOnly = false;
  $('profile-json-label').textContent = 'Parameters (JSON object)';
  value('profile-json').value = JSON.stringify(data, null, 2);
}
async function loadProfiles(announce = true) {
  const token = ++profilesGeneration;
  const navigation = libraryRevision;
  const store = value('store').value;
  const current = () => token === profilesGeneration && navigation === libraryRevision && store === value('store').value;
  const task = announce ? beginTask('profiles-status', current) : null;
  try {
    const names = await command<string[]>('profiles_list', {store});
    if (!current()) return;
    $('profile-list').innerHTML = names.map(name => `<li><button class="secondary" data-profile="${e(name)}">${e(name)}</button></li>`).join('');
    task?.message(names.length ? `${names.length} saved profiles` : 'No profiles in this store. Save a configuration to get started.');
  } catch (error) { task?.message(errorMessage(error)); }
}
async function readProfile(name: string) {
  invalidateProfileEditor();
  const revision = profileRevision;
  const navigation = libraryRevision;
  const store = value('store').value;
  profileLoading = true;
  updateProfileControls();
  const task = beginTask('profiles-status', () => revision === profileRevision && navigation === libraryRevision && store === value('store').value);
  await task.run(async () => {
    const data = await command<Json>('profiles_get', {store, name});
    if (!task.current()) return;
    value('profile-name').value = name;
    showProfileParameters(data);
    task.message('Profile loaded for review.');
  });
  if (task.current()) { profileLoading = false; updateProfileControls(); }
}
$('load-profiles').addEventListener('click', () => void loadProfiles());
$('profile-list').addEventListener('click', event => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-profile]');
  if (button) void readProfile(button.dataset.profile!);
});
$('profile-form').addEventListener('input', () => { invalidateProfileEditor(); invalidateStatus('profiles-status'); });
$('profile-form').addEventListener('change', () => { invalidateProfileEditor(); invalidateStatus('profiles-status'); });
$('store').addEventListener('input', () => {
  profilesGeneration++;
  invalidateProfileEditor();
  invalidateStatus('profiles-status');
  $('profile-list').replaceChildren();
});
function profileParameters(): Json {
  const data: unknown = JSON.parse(value('profile-json').value);
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('Profile parameters must be a JSON object.');
  return data as Json;
}
async function saveProfile() {
  if (profileMutationBusy || profileLoading) return;
  const store = value('store').value;
  const name = value('profile-name').value;
  const request = pendingProfileRequest;
  const revision = profileRevision;
  const navigation = libraryRevision;
  const current = () => revision === profileRevision && navigation === libraryRevision && store === value('store').value;
  const task = beginTask('profiles-status', current);
  profileMutationBusy = true;
  updateProfileControls();
  await task.run(async () => {
    // Capture all write inputs before yielding; edits affect the next save only.
    const parameters = request ? null : profileParameters();
    if (request) {
      await command('profiles_save_request', {store, name, request});
      if (current()) {
        const saved = await command<Json>('profiles_get', {store, name});
        if (current()) showProfileParameters(saved);
      }
    } else {
      await command('profiles_save', {store, name, parameters});
    }
    task.message(`Saved profile "${name}".`);
    if (navigation === libraryRevision && store === value('store').value) await loadProfiles(false);
  });
  profileMutationBusy = false;
  updateProfileControls();
}
$('profile-form').addEventListener('submit', event => { event.preventDefault(); void saveProfile(); });
$('save-config').addEventListener('click', () => {
  try {
    const request = currentRequest();
    pendingProfileRequest = request;
    value('profile-json').value = JSON.stringify(request, null, 2);
    $<HTMLTextAreaElement>('profile-json').readOnly = true;
    $('profile-json-label').textContent = 'Request to resolve in Rust (read-only)';
    navigate('profiles');
    value('profile-flow').value = previousFlow;
    value('profile-name').focus();
  } catch (error) { $('plan-error').textContent = errorMessage(error); }
});
$('use-profile').addEventListener('click', () => {
  try {
    if (pendingProfileRequest) throw new Error('Save this configuration first so Rust can resolve its reusable parameters.');
    const data = profileParameters();
    value('advanced').value = JSON.stringify(data, null, 2);
    let destination = value('profile-flow').value as Flow;
    if (data.capability === 'throughput') destination = 'throughput';
    if (data.capability === 'tuning') destination = 'windows_tuning';
    if (data.capability === 'path_basic' || data.capability === 'path_trace') { destination = 'path'; value('engine').value = data.capability; }
    if (data.capability === 'workflow') {
      if (typeof data.workflow === 'string' && ['triage','path','throughput','baseline','windows_tuning'].includes(data.workflow)) destination = data.workflow as Flow;
      else if (!['triage','baseline'].includes(destination)) destination = 'triage';
    }
    navigate(destination);
    loadedWorkflowEnvelope = data.capability === 'workflow';
    $('plan-state').textContent = 'Profile loaded as the final settings layer. Edit Additional settings and review a fresh plan before starting.';
  } catch (error) { $('profiles-status').textContent = errorMessage(error); }
});
$('delete-profile').addEventListener('click', () => {
  if (profileMutationBusy || profileLoading) return;
  deleteTarget = {store:value('store').value, name:value('profile-name').value, revision:profileRevision};
  $('delete-confirm').hidden = false;
});
$('keep-profile').addEventListener('click', () => { deleteTarget = null; $('delete-confirm').hidden = true; });
async function deleteProfile() {
  const target = deleteTarget;
  if (!target || profileMutationBusy || profileLoading || target.revision !== profileRevision || target.store !== value('store').value || target.name !== value('profile-name').value) return;
  const navigation = libraryRevision;
  const task = beginTask('profiles-status', () => target.revision === profileRevision && navigation === libraryRevision && target.store === value('store').value);
  deleteTarget = null;
  $('delete-confirm').hidden = true;
  profileMutationBusy = true;
  updateProfileControls();
  await task.run(async () => {
    await command('profiles_delete', {store:target.store, name:target.name});
    task.message(`Deleted profile "${target.name}".`);
    if (navigation === libraryRevision && target.store === value('store').value) await loadProfiles(false);
  });
  profileMutationBusy = false;
  updateProfileControls();
}
$('confirm-delete').addEventListener('click', () => void deleteProfile());
interface Page {rows:unknown[];metadata:Json;offset:number;next_offset:number;total:number;has_more:boolean}
interface RunsPage {runs:{path:string;summary?:Json;error?:unknown}[];total:number;has_more:boolean;legacy_index?:{path?:string;error?:unknown}}
let runsOffset = 0, rowsOffset = 0, nextRows = 0;
const rowHistory: number[] = [];
let selectedReport = '';
let reportGeneration = 0, runsGeneration = 0, comparisonGeneration = 0, exportGeneration = 0;
let exportBusy = false;
function clearComparison() {
  comparisonGeneration++;
  $('comparison').hidden = true;
  $('comparison').textContent = '';
}
function invalidateReport() {
  reportGeneration++;
  exportGeneration++;
  selectedReport = '';
  $('report-detail').hidden = true;
  clearComparison();
}
function invalidateLibraryWork() {
  libraryRevision++;
  invalidateProfileEditor();
  invalidateReport();
  invalidateStatus('profiles-status');
  invalidateStatus('reports-status');
}
async function loadRuns(offset: number) {
  const token = ++runsGeneration;
  const navigation = libraryRevision;
  const directory = value('report-directory').value;
  const task = beginTask('reports-status', () => token === runsGeneration && navigation === libraryRevision && directory === value('report-directory').value);
  await task.run(async () => {
    const data = await command<RunsPage>('runs_list', {directory, offset, limit:20});
    if (!task.current()) return;
    runsOffset = offset;
    $('run-list').innerHTML = data.runs.map(item => `<tr><td>${e(item.summary?.timestamp || item.path)}</td><td>${e(item.summary?.status || errorMessage(item.error || 'Unavailable'))}</td><td>${e(item.summary?.total ?? 'Unavailable')}</td><td><button class="secondary" data-report="${e(item.path)}">Open</button></td></tr>`).join('');
    value('runs-prev').disabled = offset === 0;
    value('runs-next').disabled = !data.has_more;
    $('runs-page').textContent = data.runs.length ? `${offset+1}–${offset+data.runs.length} of ${data.total}` : `0 of ${data.total}`;
    const status = data.runs.length ? 'Runs loaded.' : 'No recorded runs in this directory.';
    const warning = data.legacy_index?.error ? ` Legacy index ${data.legacy_index.path || 'runs.json'}: ${errorMessage(data.legacy_index.error)}` : '';
    task.message(status + warning);
  });
}
async function readReport(offset: number, path = value('report-path').value, history: 'reset'|'next'|'prev' = 'reset') {
  const token = ++reportGeneration;
  const navigation = libraryRevision;
  const inputPath = value('report-path').value;
  selectedReport = '';
  $('report-detail').hidden = true;
  clearComparison();
  exportGeneration++;
  const task = beginTask('reports-status', () => token === reportGeneration && navigation === libraryRevision && inputPath === value('report-path').value);
  await task.run(async () => {
    const data = await command<Page>('report_read', {path, offset, limit:20});
    if (!task.current()) return;
    if (history === 'next') rowHistory.push(rowsOffset);
    else if (history === 'prev') rowHistory.pop();
    else rowHistory.length = 0;
    selectedReport = path;
    rowsOffset = offset;
    nextRows = data.next_offset;
    $('report-detail').hidden = false;
    $('report-metadata').innerHTML = `<dl><dt>File</dt><dd>${e(path)}</dd><dt>Status</dt><dd>${e(data.metadata.status || 'Unavailable')}</dd><dt>Source</dt><dd>${e(data.metadata.source_schema)}</dd><dt>Provenance</dt><dd><code>${e(JSON.stringify(data.metadata.provenance))}</code></dd></dl>`;
    $('report-rows').textContent = JSON.stringify(data.rows, null, 2);
    value('rows-prev').disabled = rowHistory.length === 0;
    value('rows-next').disabled = !data.has_more;
    $('rows-page').textContent = `${offset}–${data.next_offset} of ${data.total}`;
    task.message('Report loaded.');
  });
}
$('report-directory').addEventListener('input', () => {
  runsGeneration++;
  invalidateStatus('reports-status');
  $('run-list').replaceChildren();
  $('runs-page').textContent = '';
  value('runs-prev').disabled = true;
  value('runs-next').disabled = true;
});
$('report-path').addEventListener('input', () => { invalidateReport(); invalidateStatus('reports-status'); });
$('baseline-path').addEventListener('input', () => { clearComparison(); invalidateStatus('reports-status'); });
$('export-path').addEventListener('input', () => { exportGeneration++; invalidateStatus('reports-status'); });
$('refresh-reports').addEventListener('click', () => void loadRuns(0));
$('runs-prev').addEventListener('click', () => void loadRuns(Math.max(0, runsOffset-20)));
$('runs-next').addEventListener('click', () => void loadRuns(runsOffset+20));
$('run-list').addEventListener('click', event => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-report]');
  if (button) { value('report-path').value = button.dataset.report!; void readReport(0); }
});
$('read-report').addEventListener('click', () => void readReport(0));
$('rows-next').addEventListener('click', () => void readReport(nextRows, selectedReport, 'next'));
$('rows-prev').addEventListener('click', () => void readReport(rowHistory.at(-1) || 0, selectedReport, 'prev'));
async function compareReport() {
  if (!selectedReport) return;
  const token = ++comparisonGeneration;
  const report = reportGeneration;
  const current = selectedReport;
  const baseline = value('baseline-path').value;
  const task = beginTask('reports-status', () => token === comparisonGeneration && report === reportGeneration && current === selectedReport && baseline === value('baseline-path').value);
  await task.run(async () => {
    const data = await command('report_compare', {baseline, current});
    if (!task.current()) return;
    $('comparison').hidden = false;
    $('comparison').textContent = JSON.stringify(data, null, 2);
    task.message('Comparison ready. Null values indicate unavailable evidence.');
  });
}
async function exportReport() {
  if (!selectedReport || exportBusy) return;
  const token = ++exportGeneration;
  const report = reportGeneration;
  const path = selectedReport;
  const destination = value('export-path').value;
  const task = beginTask('reports-status', () => token === exportGeneration && report === reportGeneration && path === selectedReport && destination === value('export-path').value);
  exportBusy = true;
  value('export').disabled = true;
  try {
    await task.run(async () => {
      await command('report_export', {path, destination});
      task.message('Report exported.');
    });
  } finally {
    exportBusy = false;
    value('export').disabled = false;
  }
}
$('compare').addEventListener('click', () => void compareReport());
$('export').addEventListener('click', () => void exportReport());
navigate('triage'); $('plan-state').textContent='Configure your workflow and review its plan.'; void checkRuntime(); void pollRun(); setInterval(()=>void pollRun(),250);
