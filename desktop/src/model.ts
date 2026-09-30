export type Flow = 'triage' | 'path' | 'throughput' | 'baseline' | 'windows_tuning' | 'profiles' | 'reports';
export type Json = Record<string, unknown>;
export interface Request { capability: string; layers: Json[]; strict: boolean; workflow?: string }
export interface Progress { run_id: string; state: string; completed: number; total: number; logs: string[]; exit_code: number | null; report_path: string | null }
export interface Capability { available: boolean; permission?: string; reason?: string }
export interface Doctor { os: string; architecture: string; version: string; capabilities: Record<string, Capability>; helper: Json }
export interface Inputs { host: string; family: string; round: string; engine: string; traceType: string; skip: boolean; target: string; port: number; protocol: string; maxTests: number; action: string; profile: string; udpPort: number; advanced: string; strict: boolean; backupFolder?: string; dscp?: number; powerPlan?: string; includeAppPolicies?: boolean; appPaths?: string[] }
export function requestFor(flow: Flow, input: Inputs): Request {
  const extra: unknown = JSON.parse(input.advanced || '{}');
  if (!extra || Array.isArray(extra) || typeof extra !== 'object') throw new Error('Additional settings must be a JSON object.');
  const override = extra as Json;
  const path: Json = { hostsIPv4: input.family === 'IPv4' ? [input.host] : [], hostsIPv6: input.family === 'IPv6' ? [input.host] : [], protocols: [input.family], rounds: [input.round], skipPathping: input.skip };
  const throughput = { target: input.target.trim() || null, port: input.port, protocol: input.protocol, maxTotalTests: input.maxTests };
  if (flow === 'path') return { capability: input.engine, layers: [{ ...path, ...(input.engine === 'path_trace' ? { types: [input.traceType] } : {}) }, override], strict: input.strict };
  if (flow === 'throughput') return { capability: 'throughput', layers: [throughput, override], strict: input.strict };
  if (flow === 'windows_tuning') return { capability: 'tuning', layers: [{ action: input.action, profile: input.profile, udpPorts: [input.udpPort], backupFolder: input.backupFolder?.trim() || null, dscp: input.dscp ?? 46, powerPlan: input.powerPlan || 'None', includeAppPolicies: input.includeAppPolicies ?? false, appPaths: input.appPaths || [] }, override], strict: input.strict };
  return { capability: 'workflow', workflow: flow, layers: [{ path, throughput }, override], strict: input.strict };
}
export function fingerprint(request: Request, out: string): string { return JSON.stringify([request, out]); }
export function active(run: Progress | null): boolean { return !!run && ['running', 'cancelling'].includes(run.state); }
export function errorMessage(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) return `${'category' in error ? `${error.category}: ` : ''}${error.message}`;
  return String(error);
}
export function escape(value: unknown): string { return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!); }
export function printable(value: unknown): string { return escape(JSON.stringify(value, null, 2)); }
