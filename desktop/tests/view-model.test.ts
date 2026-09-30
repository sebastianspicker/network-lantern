import { describe, it, expect } from 'vitest';
import {
  capabilityFor, capabilityNotice, disabledControls, flowEntry, helperDetail, isLibraryFlow, nonEmptyLines, parseParameters, pathRounds,
  plannedItems, profileDestination, rowsPageLabel, runSummary, runsPageLabel, runsStatus, traceTypeFor, traceTypes, visibleFieldsets,
  workflowRequest, type Doctor, type Inputs,
} from '../src/model';
import { field, planSummary, profileList, reportMetadata, runList, select } from '../src/render';

const input: Inputs = {
  host: 'localhost', family: 'IPv4', round: 'Standard', engine: 'path_basic', traceType: 'ICMP4', skip: false, target: '', port: 5201,
  protocol: 'Both', maxTests: 0, action: 'Verify', profile: 'Safe', udpPort: 5201, advanced: '{}', strict: false,
};
const doctor: Doctor = {
  os: 'test', architecture: 'test', version: 'test', helper: {},
  capabilities: { path_basic: { available: true }, path_trace: { available: false, reason: 'No trace' }, tuning: { available: true, permission: 'Admin' } },
};
const ready = { native: true, available: true, fresh: true, running: false, starting: false, planning: false, helperBusy: false };

describe('flow presentation', () => {
  it('describes flows and their visible fieldsets', () => {
    expect(flowEntry('windows_tuning').label).toBe('Windows tuning');
    expect(isLibraryFlow('reports')).toBe(true);
    expect(isLibraryFlow('baseline')).toBe(false);
    expect(visibleFieldsets('baseline')).toEqual({ 'path-fields': true, 'throughput-fields': true, 'tuning-fields': false });
    expect(visibleFieldsets('windows_tuning')).toEqual({ 'path-fields': false, 'throughput-fields': false, 'tuning-fields': true });
  });
  it('selects the capability that gates each flow', () => {
    expect(capabilityFor(null, 'path', 'path_basic')).toMatchObject({ available: false });
    expect(capabilityFor(doctor, 'path', 'path_trace')).toEqual({ available: false, reason: 'No trace' });
    expect(capabilityFor(doctor, 'triage', 'path_trace')).toEqual({ available: true });
    expect(capabilityFor(doctor, 'throughput', 'path_basic').reason).toBe('Capability information unavailable.');
    expect(capabilityNotice(doctor.capabilities.tuning!)).toBe('Admin');
    expect(capabilityNotice({ available: true })).toBe('Native runtime ready.');
  });
  it('allows starting only a fresh, available and idle preview', () => {
    expect(disabledControls(ready)).toEqual({ start: false, preview: false, helperRegister: false, helperRemove: false });
    expect(disabledControls({ ...ready, fresh: false }).start).toBe(true);
    expect(disabledControls({ ...ready, running: true })).toEqual({ start: true, preview: false, helperRegister: true, helperRemove: true });
    expect(disabledControls({ ...ready, planning: true })).toEqual({ start: true, preview: true, helperRegister: false, helperRemove: false });
    expect(disabledControls({ ...ready, native: false }).preview).toBe(true);
  });
});

describe('path options', () => {
  it('offers engine-specific rounds and family-specific trace modes', () => {
    expect(pathRounds(false)).toEqual(['Standard', 'MTU1400_DF', 'TTL64_Timeout5s']);
    expect(pathRounds(true)).toHaveLength(8);
    expect(traceTypes('IPv6')).toEqual(['ICMP6', 'TCP6', 'UDP6', 'MPLS6', 'AS6']);
    expect(traceTypeFor('TCP4', 'IPv6')).toBe('TCP6');
  });
});

describe('workflow requests and profiles', () => {
  it('replaces layers with a loaded workflow envelope only for single-capability flows', () => {
    const envelope = { ...input, advanced: '{"capability":"workflow","layers":[]}' };
    expect(workflowRequest('path', envelope, true)).toMatchObject({ capability: 'workflow', workflow: 'path', layers: [JSON.parse(envelope.advanced)] });
    expect(workflowRequest('triage', envelope, true).layers).toHaveLength(2);
    expect(workflowRequest('path', envelope, false).capability).toBe('path_basic');
  });
  it('parses profile parameters as JSON objects and trims application paths', () => {
    expect(parseParameters('{"a":1}')).toEqual({ a: 1 });
    for (const text of ['[]', 'null', '1']) expect(() => parseParameters(text)).toThrow('Profile parameters must be a JSON object.');
    expect(nonEmptyLines(' a \n\n b')).toEqual(['a', 'b']);
  });
  it('chooses the workflow that a profile loads into', () => {
    expect(profileDestination({ capability: 'tuning' }, 'triage')).toEqual({ flow: 'windows_tuning' });
    expect(profileDestination({ capability: 'path_trace' }, 'triage')).toEqual({ flow: 'path', engine: 'path_trace' });
    expect(profileDestination({ capability: 'workflow', workflow: 'throughput' }, 'triage')).toEqual({ flow: 'throughput' });
    expect(profileDestination({ capability: 'workflow', workflow: 'reports' }, 'path')).toEqual({ flow: 'triage' });
    expect(profileDestination({ capability: 'workflow' }, 'baseline')).toEqual({ flow: 'baseline' });
    expect(profileDestination({ host: 'x' }, 'path')).toEqual({ flow: 'path' });
  });
});

describe('run and report summaries', () => {
  it('summarizes run progress with bounded activity', () => {
    const logs = Array.from({ length: 70 }, (_, n) => `line ${n}`);
    const summary = runSummary({ run_id: 'r', state: 'partial_failure', completed: 2, total: 5, logs, exit_code: null, report_path: null });
    expect(summary.title).toBe('partial failure');
    expect(summary.count).toBe('2 of 5 measurements · r');
    expect(summary.logs.split('\n')).toHaveLength(64);
    expect(runSummary({ run_id: 'r', state: 'running', completed: 0, total: 0, logs: [], exit_code: null, report_path: null }).logs)
      .toBe('No activity recorded yet.');
  });
  it('describes helper status and paging', () => {
    expect(helperDetail({ registration: 'registered' })).toBe('registered');
    expect(helperDetail({ error: { category: 'internal', message: 'Down' } })).toBe('internal: Down');
    expect(helperDetail({})).toBe('Helper status unavailable');
    expect(runsPageLabel(20, 3, 23)).toBe('21–23 of 23');
    expect(runsPageLabel(0, 0, 0)).toBe('0 of 0');
    expect(rowsPageLabel(0, 20, 25)).toBe('0–20 of 25');
    expect(runsStatus({ runs: [], total: 0, has_more: false, legacy_index: { error: { message: 'bad' } } }))
      .toBe('No recorded runs in this directory. Legacy index runs.json: bad');
  });
  it('reports planned item counts from the most specific source', () => {
    expect(plannedItems({ plan: { total_tests: 3, total_items: 1 } })).toBe(3);
    expect(plannedItems({ total_items: 0 })).toBe(0);
    expect(plannedItems({})).toBe('See resolved plan');
  });
});

describe('markup fragments escape external values', () => {
  const hostile = '"><img src=x onerror=alert(1)>';
  it('escapes form field values and attributes', () => {
    expect(field('x', 'Label', hostile, 'text', { placeholder: hostile, required: true }))
      .toBe('<label class="field" for="x"><span>Label</span><input id="x" type="text" '
        + 'value="&quot;&gt;&lt;img src=x onerror=alert(1)&gt;" placeholder="&quot;&gt;&lt;img src=x onerror=alert(1)&gt;" required></label>');
    expect(select('s', 'S', ['<a>', { value: 'v"', label: '<b>' }])).toContain('<option>&lt;a&gt;</option><option value="v&quot;">&lt;b&gt;</option>');
  });
  it('escapes plan, profile, run and report data', () => {
    for (const markup of [
      planSummary({ capability: hostile, warnings: [hostile] }),
      profileList([hostile]),
      runList([{ path: hostile, summary: { status: hostile } }]),
      reportMetadata(hostile, { status: hostile, source_schema: hostile, provenance: { x: hostile } }),
    ]) expect(markup).not.toContain('<img');
  });
});
