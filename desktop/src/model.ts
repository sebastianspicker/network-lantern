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
  // One-sentence lede shown under the workflow title.
  summary: string;
  group: 'measure' | 'change' | 'records';
}

export const flows: readonly FlowEntry[] = [
  {
    id: 'triage', label: 'Triage', description: 'Path, then throughput', group: 'measure',
    summary: 'Trace the route to a host, then measure throughput against an iperf3 server you control.',
  },
  {
    id: 'path', label: 'Path', description: 'Reachability and routing', group: 'measure',
    summary: 'Ping, trace and TCP 443 checks to one host, with the basic suite or a continuous MTR-style trace.',
  },
  {
    id: 'throughput', label: 'Throughput', description: 'iperf3 test matrix', group: 'measure',
    summary: 'An iperf3-compatible matrix across protocols, streams, windows and DSCP classes. Check the test count before you start.',
  },
  {
    id: 'baseline', label: 'Baseline', description: 'Path and one sample', group: 'measure',
    summary: 'Path evidence plus a single throughput test: a reference point to compare later runs against.',
  },
  {
    id: 'windows_tuning', label: 'Windows tuning', description: 'Verify, back up, apply, restore', group: 'change',
    summary: 'Verify inspects the managed Windows QoS and power settings. Backup, Apply and Restore change system state.',
  },
  {
    id: 'profiles', label: 'Profiles', description: 'Saved configurations', group: 'records',
    summary: 'Settings you reuse, kept in a local store. A loaded profile still needs a fresh plan.',
  },
  {
    id: 'reports', label: 'Reports', description: 'Recorded evidence', group: 'records',
    summary: 'Runs recorded in a results directory. Page through measurements, compare with a baseline, or export.',
  },
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

export type Tone = 'alert' | 'ok' | 'lamp';

export interface PlanFact {
  label: string;
  value: string;
  tone?: Tone;
  // Spans the full row (host lists, server:port).
  wide?: boolean;
}

export interface PlanEntry {
  title: string;
  facts: PlanFact[];
  // Ordered operations, when the capability reports them (Windows tuning).
  operations: string[];
  warnings: string[];
}

const capabilityTitles: Record<string, string> = {
  path_basic: 'Path · basic diagnostics',
  path_trace: 'Path · continuous trace',
  throughput: 'Throughput matrix',
  tuning: 'Windows tuning',
  workflow: 'Workflow',
};

const strings = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const record = (value: unknown): Json => (value && typeof value === 'object' && !Array.isArray(value) ? value as Json : {});
const isCount = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0;

export function countLabel(count: number, singular: string, plural = `${singular}s`): string {
  return `${count.toLocaleString('en-US')} ${count === 1 ? singular : plural}`;
}

// Nominal duration as the Rust planner estimates it; rounded for reading, never presented as exact.
export function formatDuration(seconds: number): string {
  if (seconds < 90) return `${Math.max(1, Math.round(seconds))} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} h ${rest} min` : `${hours} h`;
}

const acronyms: Record<string, string> = { qos: 'QoS', dscp: 'DSCP', udp: 'UDP', tcp: 'TCP', mtu: 'MTU', ttl: 'TTL' };

export function sentenceCase(kind: string): string {
  const words = kind.replaceAll('_', ' ').trim().replace(/\b[a-z]+\b/g, word => acronyms[word] ?? word);
  return words.charAt(0).toUpperCase() + words.slice(1);
}

// Hosts the plan names for the selected address families; throughput names its server and port.
export function planTargets(step: Json): string[] {
  const plan = record(step.plan);
  const config = record(plan.config);
  if (typeof config.target === 'string' && config.target) {
    return [isCount(config.port) ? `${config.target}:${config.port}` : config.target];
  }
  const families = strings(plan.families);
  const ipv4 = strings(plan.ipv4_hosts);
  const ipv6 = strings(plan.ipv6_hosts);
  if (!families.length) return [...ipv4, ...ipv6];
  return [...(families.includes('ipv4') ? ipv4 : []), ...(families.includes('ipv6') ? ipv6 : [])];
}

export function planEntry(step: Json): PlanEntry {
  const plan = record(step.plan);
  const capability = String(step.capability ?? '');
  const facts: PlanFact[] = [];
  // The most consequential fact leads: whether this run changes the system.
  if (typeof plan.mutating === 'boolean') {
    facts.push(plan.mutating
      ? { label: 'Windows settings', value: 'Changed by this run', tone: 'alert', wide: true }
      : { label: 'Windows settings', value: 'Read only, nothing is changed', tone: 'ok', wide: true });
  }
  const targets = planTargets(step);
  if (targets.length) facts.push({ label: targets.length === 1 ? 'Target' : 'Targets', value: targets.join('\n'), wide: true });

  const items = plannedItems(step);
  if (isCount(plan.total_tests)) facts.push({ label: 'Planned', value: countLabel(plan.total_tests, 'test') });
  else if (isCount(items)) facts.push({ label: 'Planned', value: countLabel(items, 'planned item') });
  else facts.push({ label: 'Planned', value: String(items) });

  if (isCount(plan.estimated_test_seconds)) facts.push({ label: 'Nominal time', value: `≈ ${formatDuration(plan.estimated_test_seconds)}` });
  if (typeof plan.within_budget === 'boolean') {
    const limit = isCount(plan.max_total_tests) ? plan.max_total_tests : null;
    if (!plan.within_budget) facts.push({ label: 'Budget', value: limit === null ? 'Over budget' : `Over the ${countLabel(limit, 'test')} limit`, tone: 'alert' });
    else facts.push({ label: 'Budget', value: limit === null ? 'No limit set' : `Within ${countLabel(limit, 'test')}` });
  }
  const rounds = strings(plan.rounds);
  if (rounds.length) facts.push({ label: rounds.length === 1 ? 'Round' : 'Rounds', value: rounds.join(', ') });
  if (capability === 'path_trace') {
    const types = strings(plan.types);
    if (types.length) facts.push({ label: 'Probe types', value: types.join(', ') });
  }

  const config = record(plan.config);
  if (capability === 'tuning' && typeof config.action === 'string') {
    facts.push({ label: 'Action', value: typeof config.profile === 'string' ? `${config.action} · ${config.profile}` : config.action });
  }
  if (typeof plan.requiresHelper === 'boolean') {
    facts.push({ label: 'Privileged helper', value: plan.requiresHelper ? 'Required' : 'Not required' });
  }

  const operations = (Array.isArray(plan.steps) ? plan.steps : []).map(record).filter(item => typeof item.kind === 'string')
    .map(item => sentenceCase(item.kind as string) + (isCount(item.count) ? ` (${item.count.toLocaleString('en-US')})` : ''));
  return { title: capabilityTitles[capability] ?? (capability ? sentenceCase(capability) : 'Plan'), facts, operations, warnings: strings(step.warnings) };
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
