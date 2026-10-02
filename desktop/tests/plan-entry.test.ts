import { describe, it, expect } from 'vitest';
import { formatDuration, planEntry, planTargets, sentenceCase } from '../src/model';
import { planSummary } from '../src/render';

// Shapes taken from `network-lantern workflow ... --dry-run` output.
const pathStep = {
  capability: 'path_basic', warnings: [],
  plan: { families: ['ipv4'], ipv4_hosts: ['example.com'], ipv6_hosts: ['cloudflare.com'], rounds: ['Standard'], total_items: 1 },
};
const throughputStep = {
  capability: 'throughput', warnings: [],
  plan: { config: { target: 'iperf3.example.net', port: 5201 }, total_tests: 1145, estimated_test_seconds: 12595, max_total_tests: null, within_budget: true },
};
const tuningStep = {
  capability: 'tuning', warnings: [],
  plan: { config: { action: 'Apply', profile: 'Safe' }, mutating: true, requiresHelper: true,
    steps: [{ kind: 'write_and_verify_backup' }, { kind: 'enable_local_qos' }, { kind: 'reconcile_port_policies', count: 1 }] },
};
const fact = (step: object, label: string) => planEntry(step as never).facts.find(item => item.label === label);

describe('plan manifest', () => {
  it('names only hosts in the selected families, and the throughput server with its port', () => {
    expect(planTargets(pathStep)).toEqual(['example.com']);
    expect(planTargets(throughputStep)).toEqual(['iperf3.example.net:5201']);
    expect(planTargets({ plan: { ipv4_hosts: ['a.example'], ipv6_hosts: [] } })).toEqual(['a.example']);
  });
  it('states test count, nominal time and budget from the Rust plan', () => {
    expect(fact(throughputStep, 'Planned')?.value).toBe('1,145 tests');
    expect(fact(throughputStep, 'Nominal time')?.value).toBe('≈ 3 h 30 min');
    expect(fact(throughputStep, 'Budget')?.value).toBe('No limit set');
    expect(fact({ capability: 'throughput', plan: { total_tests: 9, within_budget: false, max_total_tests: 4 } }, 'Budget'))
      .toMatchObject({ value: 'Over the 4 tests limit', tone: 'alert' });
  });
  it('leads with the Windows change and lists ordered operations', () => {
    const entry = planEntry(tuningStep);
    expect(entry.facts[0]).toMatchObject({ label: 'Windows settings', value: 'Changed by this run', tone: 'alert' });
    expect(fact(tuningStep, 'Privileged helper')?.value).toBe('Required');
    expect(entry.operations).toEqual(['Write and verify backup', 'Enable local QoS', 'Reconcile port policies (1)']);
  });
  it('falls back without inventing values', () => {
    const entry = planEntry({ capability: 'workflow', plan: {} });
    expect(entry.facts).toEqual([{ label: 'Planned', value: 'See resolved plan' }]);
    expect(planEntry({ capability: 'tuning', plan: {}, total_items: 1 }).facts).toEqual([{ label: 'Planned', value: '1 planned item' }]);
  });
  it('formats nominal durations for reading', () => {
    expect([30, 89, 90, 3599, 3600, 12595].map(formatDuration)).toEqual(['30 s', '89 s', '2 min', '1 h', '1 h', '3 h 30 min']);
    expect(sentenceCase('validate_backup_destination')).toBe('Validate backup destination');
  });
  it('escapes plan values in the rendered manifest', () => {
    const html = planSummary({ capability: 'path_trace', plan: { ipv4_hosts: ['<img src=x>'] }, warnings: ['<b>'] });
    expect(html).not.toContain('<img');
    expect(html).not.toContain('<b>');
    expect(html).toContain('Path · continuous trace');
  });
});
