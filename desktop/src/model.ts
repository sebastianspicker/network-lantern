export type Flow = 'triage' | 'path' | 'throughput' | 'baseline' | 'windows_tuning' | 'profiles' | 'reports';
export type Json = Record<string, unknown>;

export interface Request {
  capability: string;
  layers: Json[];
  strict: boolean;
  workflow?: string;
}

export interface Progress {
  run_id: string;
  state: string;
  completed: number;
  total: number;
  logs: string[];
  exit_code: number | null;
  report_path: string | null;
}

export interface Capability {
  available: boolean;
  permission?: string;
  reason?: string;
}

export interface Doctor {
  os: string;
  architecture: string;
  version: string;
  capabilities: Record<string, Capability>;
  helper: Json;
}

export interface Inputs {
  host: string;
  family: string;
  round: string;
  engine: string;
  traceType: string;
  skip: boolean;
  target: string;
  port: number;
  protocol: string;
  maxTests: number;
  action: string;
  profile: string;
  udpPort: number;
  advanced: string;
  strict: boolean;
  backupFolder?: string;
  dscp?: number;
  powerPlan?: string;
  includeAppPolicies?: boolean;
  appPaths?: string[];
}

export function requestFor(flow: Flow, input: Inputs): Request {
  const extra: unknown = JSON.parse(input.advanced || '{}');
  if (!extra || Array.isArray(extra) || typeof extra !== 'object') throw new Error('Additional settings must be a JSON object.');
  const override = extra as Json;
  const path: Json = {
    hostsIPv4: input.family === 'IPv4' ? [input.host] : [],
    hostsIPv6: input.family === 'IPv6' ? [input.host] : [],
    protocols: [input.family],
    rounds: [input.round],
    skipPathping: input.skip,
  };
  const throughput = { target: input.target.trim() || null, port: input.port, protocol: input.protocol, maxTotalTests: input.maxTests };
  if (flow === 'path') {
    const trace = input.engine === 'path_trace' ? { types: [input.traceType] } : {};
    return { capability: input.engine, layers: [{ ...path, ...trace }, override], strict: input.strict };
  }
  if (flow === 'throughput') return { capability: 'throughput', layers: [throughput, override], strict: input.strict };
  if (flow === 'windows_tuning') {
    const tuning: Json = {
      action: input.action,
      profile: input.profile,
      udpPorts: [input.udpPort],
      backupFolder: input.backupFolder?.trim() || null,
      dscp: input.dscp ?? 46,
      powerPlan: input.powerPlan || 'None',
      includeAppPolicies: input.includeAppPolicies ?? false,
      appPaths: input.appPaths || [],
    };
    return { capability: 'tuning', layers: [tuning, override], strict: input.strict };
  }
  return { capability: 'workflow', workflow: flow, layers: [{ path, throughput }, override], strict: input.strict };
}

export function fingerprint(request: Request, out: string): string {
  return JSON.stringify([request, out]);
}

export function active(run: Progress | null): boolean {
  return !!run && ['running', 'cancelling'].includes(run.state);
}

export function errorMessage(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    return `${'category' in error ? `${error.category}: ` : ''}${error.message}`;
  }
  return String(error);
}

const entities: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

export function escape(value: unknown): string {
  return String(value ?? '').replace(/[&<>"']/g, c => entities[c]!);
}

export interface FlowEntry {
  id: Flow;
  label: string;
  description: string;
}

export const flows: readonly FlowEntry[] = [
  { id: 'triage', label: 'Triage', description: 'Path and throughput together' },
  { id: 'path', label: 'Path', description: 'Reachability and routing' },
  { id: 'throughput', label: 'Throughput', description: 'Measure a test matrix' },
  { id: 'baseline', label: 'Baseline', description: 'Path and one sample' },
  { id: 'windows_tuning', label: 'Windows tuning', description: 'Review and recover settings' },
  { id: 'profiles', label: 'Profiles', description: 'Reusable configurations' },
  { id: 'reports', label: 'Reports', description: 'Browse and compare evidence' },
];

export const measurementFlows: Flow[] = ['triage', 'path', 'throughput', 'baseline', 'windows_tuning'];

export function flowEntry(flow: Flow): FlowEntry {
  return flows.find(({ id }) => id === flow)!;
}

export function isLibraryFlow(flow: Flow): boolean {
  return flow === 'profiles' || flow === 'reports';
}

export function visibleFieldsets(flow: Flow): Record<'path-fields' | 'throughput-fields' | 'tuning-fields', boolean> {
  return {
    'path-fields': ['triage', 'path', 'baseline'].includes(flow),
    'throughput-fields': ['triage', 'throughput', 'baseline'].includes(flow),
    'tuning-fields': flow === 'windows_tuning',
  };
}

export function capabilityId(flow: Flow, engine: string): string {
  if (flow === 'throughput') return 'throughput';
  if (flow === 'windows_tuning') return 'tuning';
  return flow === 'path' ? engine : 'path_basic';
}

export function capabilityFor(doctor: Doctor | null, flow: Flow, engine: string): Capability {
  if (!doctor) return { available: false, reason: 'Native runtime unavailable. Open the desktop application.' };
  return doctor.capabilities[capabilityId(flow, engine)] || { available: false, reason: 'Capability information unavailable.' };
}

export function capabilityNotice(capability: Capability): string {
  return capability.reason || capability.permission || 'Native runtime ready.';
}

export interface ControlState {
  native: boolean;
  available: boolean;
  fresh: boolean;
  running: boolean;
  starting: boolean;
  planning: boolean;
  helperBusy: boolean;
}

export function disabledControls(s: ControlState) {
  const helper = !s.native || s.running || s.starting || s.helperBusy;
  return {
    start: !s.native || !s.available || !s.fresh || s.running || s.starting || s.planning || s.helperBusy,
    preview: !s.native || s.planning || s.starting || s.helperBusy,
    helperRegister: helper,
    helperRemove: helper,
  };
}

// A loaded workflow profile replaces the generated layers with the full envelope for single-capability flows.
export function workflowRequest(flow: Flow, input: Inputs, loadedWorkflowEnvelope: boolean): Request {
  const request = requestFor(flow, input);
  if (loadedWorkflowEnvelope && ['path', 'throughput', 'windows_tuning'].includes(flow)) {
    request.capability = 'workflow';
    request.workflow = flow;
    request.layers = [JSON.parse(input.advanced) as Json];
  }
  return request;
}

export function nonEmptyLines(text: string): string[] {
  return text.split('\n').map(line => line.trim()).filter(Boolean);
}

export function pathRounds(trace: boolean): string[] {
  return trace
    ? ['Standard', 'MTU1400', 'TOS_CS5', 'TOS_AF11', 'TTL10', 'TTL64', 'FirstTTL3', 'Timeout5']
    : ['Standard', 'MTU1400_DF', 'TTL64_Timeout5s'];
}

export function traceTypes(family: string): string[] {
  const suffix = family === 'IPv4' ? '4' : '6';
  return ['ICMP', 'TCP', 'UDP', 'MPLS', 'AS'].map(type => type + suffix);
}

export function traceTypeFor(prior: string, family: string): string {
  return prior.replace(/[46]$/, '') + (family === 'IPv4' ? '4' : '6');
}

export function parseParameters(text: string): Json {
  const data: unknown = JSON.parse(text);
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('Profile parameters must be a JSON object.');
  return data as Json;
}

export function profileDestination(data: Json, selected: Flow): { flow: Flow; engine?: string } {
  if (data.capability === 'throughput') return { flow: 'throughput' };
  if (data.capability === 'tuning') return { flow: 'windows_tuning' };
  if (data.capability === 'path_basic' || data.capability === 'path_trace') return { flow: 'path', engine: data.capability };
  if (data.capability === 'workflow') {
    if (typeof data.workflow === 'string' && (measurementFlows as string[]).includes(data.workflow)) return { flow: data.workflow as Flow };
    if (!['triage', 'baseline'].includes(selected)) return { flow: 'triage' };
  }
  return { flow: selected };
}

export function plannedItems(step: Json): unknown {
  const plan = step.plan as Json | undefined;
  return plan?.total_tests ?? plan?.total_items ?? step.total_items ?? 'See resolved plan';
}

export function runSummary(run: Progress) {
  return {
    title: run.state.replaceAll('_', ' '),
    count: `${run.completed} of ${run.total} measurements · ${run.run_id}`,
    logs: run.logs.slice(-64).join('\n') || 'No activity recorded yet.',
  };
}

export function helperDetail(helper: Json): string {
  return String(helper.detail || helper.registration || (helper.error ? errorMessage(helper.error) : 'Helper status unavailable'));
}

export interface ReportPage {
  rows: unknown[];
  metadata: Json;
  offset: number;
  next_offset: number;
  total: number;
  has_more: boolean;
}

export interface RunsPage {
  runs: { path: string; summary?: Json; error?: unknown }[];
  total: number;
  has_more: boolean;
  legacy_index?: { path?: string; error?: unknown };
}

export function runsPageLabel(offset: number, count: number, total: number): string {
  return count ? `${offset + 1}–${offset + count} of ${total}` : `0 of ${total}`;
}

export function runsStatus(page: RunsPage): string {
  const status = page.runs.length ? 'Runs loaded.' : 'No recorded runs in this directory.';
  const legacy = page.legacy_index;
  const warning = legacy?.error ? ` Legacy index ${legacy.path || 'runs.json'}: ${errorMessage(legacy.error)}` : '';
  return status + warning;
}

export function rowsPageLabel(offset: number, next: number, total: number): string {
  return `${offset}–${next} of ${total}`;
}
