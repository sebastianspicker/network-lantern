const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { test } = require('node:test');
const { buildPlan, defaults, workflows } = require('../../site/planner.js');
const html = readFileSync(resolve(__dirname, '../../site/index.html'), 'utf8');
const options = Object.fromEntries([...html.matchAll(/<select\b[^>]*id="([^"]+)"[^>]*>(.*?)<\/select>/g)].map(([, id, content]) => [id, [...content.matchAll(/<option>(.*?)<\/option>/g)].map((m) => m[1])]));

for (const workflow of Object.keys(workflows)) {
  for (const skip of [false, true]) {
    test(`${workflow}: every select option, skip pathping ${skip}`, () => {
      for (const protocol of options.protocol) {
        for (const round of options.round) {
          for (const throughputProtocol of options['throughput-protocol']) {
            for (const action of options['tuning-action']) {
              for (const profile of options['tuning-profile']) {
                const plan = buildPlan(workflow, { ...defaults, skip, protocol, round, throughputProtocol, action, profile });
                assert.deepEqual(plan.errors, {});
                const path = ['Path', 'Triage', 'Baseline'].includes(workflow);
                const throughput = ['Throughput', 'Triage', 'Baseline'].includes(workflow);
                assert.match(plan.command, /^network-lantern workflow [a-z-]+ --settings '/);
                assert.match(plan.command, /' --dry-run$/);
                const settings = JSON.parse(plan.command.split(" --settings '")[1].split("' --dry-run")[0]);
                if (path) assert.deepEqual(settings.path, { [`hosts${protocol}`]: ['example.com'], protocols: [protocol], rounds: [round], skipPathping: skip });
                if (throughput) assert.deepEqual(settings.throughput, { target: 'iperf3.example.net', port: 5201, protocol: throughputProtocol, maxTotalTests: 0 });
                if (workflow === 'WindowsTuning') assert.deepEqual(settings.windowsTuning, { action, profile, udpPorts: [5201] });
                if (path) assert.equal(plan.steps[0].description.includes('pathping'), !skip);
                assert.equal(plan.steps.length, workflow === 'Triage' || workflow === 'Baseline' ? 2 : 1);
              }
            }
          }
        }
      }
    });
  }
}

test('rejects shell fragments, URLs, empty hosts, malformed addresses, and family mismatches', () => {
  for (const host of ['', "server'; exit 0; #", '$(whoami)', 'https://example.com', 'example.com:443', 'a\n-DryRun:$false', 'a..b', '-server', '256.1.1.1', '2001:db8::1']) {
    const plan = buildPlan('Path', { ...defaults, host });
    assert.ok(plan.errors.host, host);
    assert.equal(plan.command, '');
  }
  assert.ok(buildPlan('Path', { ...defaults, protocol: 'IPv6', host: '192.0.2.1' }).errors.host);
  assert.ok(buildPlan('Path', { ...defaults, protocol: 'IPv6', host: '2001:db8::1' }).command.includes('"hostsIPv6":["2001:db8::1"]'));
  assert.ok(buildPlan('Path', { ...defaults, host: 'router.example.' }).command);
});

test('validates active numeric fields and keeps over-budget previews available', () => {
  for (const port of ['', '0', '65536', '1.5', '-1', 'NaN', '1e3', '2;exit']) {
    assert.ok(buildPlan('Throughput', { ...defaults, port }).errors['iperf-port']);
    assert.ok(buildPlan('WindowsTuning', { ...defaults, udp: port }).errors['udp-port']);
  }
  for (const maxTests of ['0', '1', '1000000']) {
    assert.ok(buildPlan('Triage', { ...defaults, maxTests }).command.includes(`"maxTotalTests":${maxTests}`));
  }
  for (const maxTests of ['', '-1', '1.5', '1000001', '1e3']) {
    assert.ok(buildPlan('Throughput', { ...defaults, maxTests }).errors['max-tests']);
  }
  assert.ok(buildPlan('Path', { ...defaults, target: '', port: '', maxTests: '' }).command);
  assert.ok(buildPlan('WindowsTuning', { ...defaults, host: '' }).command);
});

test('describes single-test baseline and restore requirements accurately', () => {
  const baseline = buildPlan('Baseline', { ...defaults, throughputProtocol: 'Both' });
  assert.match(baseline.steps[1].description, /One TCP transmit test/);
  assert.match(buildPlan('Baseline', { ...defaults, throughputProtocol: 'UDP' }).steps[1].description, /One UDP transmit test/);
  assert.match(buildPlan('WindowsTuning', { ...defaults, action: 'Restore' }).context, /existing, trusted backup/);
});

test('normalizes surrounding whitespace without falling back from an invalid empty input', () => {
  assert.match(buildPlan('Throughput', { ...defaults, target: ' example.net ', port: ' 5202 ' }).command, /"target":"example.net"/);
  assert.equal(buildPlan('Throughput', { ...defaults, target: ' ' }).command, '');
  assert.equal(buildPlan('Throughput', { ...defaults, target: 'example.net.' }).command, '');
});

test('rejects unknown workflows and manipulated select values', () => {
  assert.throws(() => buildPlan('ArbitraryCommand', defaults), /Unknown workflow/);
  for (const input of [{ protocol: 'IPv5' }, { round: 'Reset' }, { throughputProtocol: 'SCTP' }]) {
    assert.equal(buildPlan('Triage', { ...defaults, ...input }).command, '');
  }
  assert.equal(buildPlan('WindowsTuning', { ...defaults, action: 'Execute' }).command, '');
});
