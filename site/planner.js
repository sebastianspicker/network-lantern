/* Pure command planning. This file never executes commands or accesses a service. */
const LanternPlanner = (() => {
  const docs = 'https://github.com/sebastianspicker/network-lantern/blob/main/';
  const workflows = {
    Triage: { title: 'Triage', description: 'Path evidence, followed by a throughput matrix.', path: true, throughput: true, guide: 'docs/architecture.md#workflow-composition' },
    Path: { title: 'Path', description: 'Trace reachability and routing for a selected host.', path: true, guide: 'docs/workflows/diagnose-path.md' },
    Throughput: { title: 'Throughput', description: 'Configure a throughput matrix for a trusted iperf3 server.', throughput: true, guide: 'docs/workflows/diagnose-throughput.md' },
    Baseline: { title: 'Baseline', description: 'Path evidence, followed by one throughput sample.', path: true, throughput: true, guide: 'docs/architecture.md#workflow-composition' },
    WindowsTuning: { title: 'Windows tuning', description: 'Inspect a Windows tuning action before changing settings.', tuning: true, guide: 'docs/workflows/windows-tuning.md' },
  };
  const defaults = Object.freeze({ host: 'example.com', protocol: 'IPv4', round: 'Standard', skip: false, target: 'iperf3.example.net', port: '5201', throughputProtocol: 'Both', maxTests: '0', action: 'Verify', profile: 'Safe', udp: '5201' });
  const quote = (value) => `'${String(value).replaceAll("'", "''")}'`;
  const integer = (value, minimum, maximum) => /^\d+$/.test(String(value)) && Number(value) >= minimum && Number(value) <= maximum;

  function hostIsValid(host, protocol) {
    if (!host || host.length > 253) return false;
    // The throughput module requires hostnames without a trailing root dot.
    if (!protocol && host.endsWith('.')) return false;
    if (host.includes(':')) {
      if (protocol === 'IPv4') return false;
      try { return new URL(`http://[${host}]/`).hostname.startsWith('['); } catch { return false; }
    }
    if (/^[\d.]+$/.test(host)) {
      return protocol !== 'IPv6' && host.split('.').length === 4 && host.split('.').every((part) => integer(part, 0, 255));
    }
    return host.replace(/\.$/, '').split('.').every((label) => /^[a-z\d](?:[a-z\d-]{0,61}[a-z\d])?$/i.test(label));
  }

  function buildPlan(workflow, input) {
    const definition = workflows[workflow];
    if (!definition) throw new Error('Unknown workflow.');
    const v = Object.fromEntries(Object.entries({ ...defaults, ...input }).map(([key, value]) => [key, typeof value === 'string' ? value.trim() : value]));
    const errors = {};
    if (definition.path) {
      if (!hostIsValid(v.host, v.protocol)) errors.host = `Enter a hostname or ${v.protocol} address, without a URL or port.`;
      if (!['IPv4', 'IPv6'].includes(v.protocol)) errors.protocol = 'Choose IPv4 or IPv6.';
      if (!['Standard', 'MTU1400_DF', 'TTL64_Timeout5s'].includes(v.round)) errors.round = 'Choose a supported diagnostic round.';
    }
    if (definition.throughput) {
      if (!hostIsValid(v.target)) errors['iperf-target'] = 'Enter a hostname or IP address, without a URL or port.';
      if (!integer(v.port, 1, 65535)) errors['iperf-port'] = 'Use a whole port number from 1 to 65535.';
      if (!['Both', 'TCP', 'UDP'].includes(v.throughputProtocol)) errors['throughput-protocol'] = 'Choose Both, TCP, or UDP.';
      if (!integer(v.maxTests, 0, 1000000)) errors['max-tests'] = 'Use a whole number from 0 to 1,000,000.';
    }
    if (definition.tuning) {
      if (!['Verify', 'Apply', 'Backup', 'Restore'].includes(v.action)) errors['tuning-action'] = 'Choose a supported tuning action.';
      if (!['Safe', 'Measured'].includes(v.profile)) errors['tuning-profile'] = 'Choose Safe or Measured.';
      if (!integer(v.udp, 1, 65535)) errors['udp-port'] = 'Use a whole port number from 1 to 65535.';
    }

    const settings = {};
    const commandWorkflow = workflow === 'WindowsTuning' ? 'windows-tuning' : workflow.toLowerCase();
    const steps = [];
    if (definition.path) {
      settings.path = { [`hosts${v.protocol}`]: [v.host], protocols: [v.protocol], rounds: [v.round], skipPathping: v.skip };
      steps.push({ title: 'Collect path evidence', description: `Ping, trace${v.skip ? '' : ', pathping'} and TCP 443. ${v.protocol} · ${v.round}.` });
    }
    if (definition.throughput) {
      settings.throughput = { target: v.target, port: Number(v.port), protocol: v.throughputProtocol, maxTotalTests: Number(v.maxTests) };
      const protocol = v.throughputProtocol === 'Both' ? 'Both protocols' : `${v.throughputProtocol} protocol`;
      const sampleProtocol = v.throughputProtocol === 'UDP' ? 'UDP' : 'TCP';
      steps.push({ title: workflow === 'Baseline' ? 'Plan one throughput sample' : 'Plan throughput tests', description: workflow === 'Baseline' ? `One ${sampleProtocol} transmit test. The workflow selects single-test mode.` : `${protocol} · default matrix. ${Number(v.maxTests) === 0 ? 'No test budget set.' : `Live runs limited to ${Number(v.maxTests).toLocaleString('en-US')} planned ${Number(v.maxTests) === 1 ? 'measurement' : 'measurements'}.`}` });
    }
    if (definition.tuning) {
      settings.windowsTuning = { action: v.action, profile: v.profile, udpPorts: [Number(v.udp)] };
      const descriptions = {
        Verify: 'Preview the verification plan. No Windows settings are read by this browser.',
        Apply: `Preview the ${v.profile} settings plan. No changes are applied.`,
        Backup: 'Preview backup creation. No backup is written in a dry run.',
        Restore: 'Preview the ordered recovery steps. Backup integrity is checked before an actual restore.',
      };
      steps.push({ title: `Preview ${v.action.toLowerCase()}`, description: descriptions[v.action] });
    }
    const command = `network-lantern workflow ${commandWorkflow} --settings ${quote(JSON.stringify(settings))} --dry-run`;
    const context = definition.tuning
      ? (v.action === 'Restore' ? 'Restore needs an existing, trusted backup on the Windows host. Dry run plans the recovery steps without reading the backup.' : 'The generated command previews this action. Live Windows changes require a trusted checkout and a verified recovery path.')
      : (definition.throughput ? 'Use an operator-controlled iperf3 server. The local preview calculates the actual test count, nominal duration, and budget status.' : 'The local preview validates the selected host, protocol, and round. Collect diagnostics only from an authorized host.');
    return { ...definition, guide: docs + definition.guide, errors, steps, context, command: Object.keys(errors).length ? '' : command };
  }
  return { workflows, defaults, buildPlan };
})();
if (typeof module !== 'undefined' && module.exports) module.exports = LanternPlanner;
