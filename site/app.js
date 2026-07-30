(() => {
  const q = (selector) => document.querySelector(selector);
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  const controls = { path: q('#path-controls'), throughput: q('#throughput-controls'), tuning: q('#tuning-controls') };
  const copy = {
    Triage: { description: 'Collect path evidence, then add the configured throughput matrix when a trusted target is supplied.', summary: 'The triage preview starts with path evidence and appends the configured throughput matrix preview.', steps: ['Path diagnostics: ping, tracert, pathping, and TCP 443 check for the selected host.', 'Preview the configured throughput matrix against the supplied iperf3 target.', 'No artifact directories, network connections, or output files are created.'] },
    Path: { description: 'Preview the Windows path diagnostics matrix for one selected host, protocol, and round.', summary: 'The path preview shows the selected diagnostic sequence without contacting the target.', steps: ['Validate the selected host, protocol, and round.', 'Plan ping, tracert, optional pathping, and TCP 443 diagnostics.', 'No probes run and no JSON or CSV result files are written.'] },
    Throughput: { description: 'Preview the configured iperf3 throughput matrix against a trusted target.', summary: 'The throughput preview is a matrix plan only; it contains no measurements.', steps: ['Validate the iperf3 target, port, and selected protocol.', 'Plan the configured throughput matrix invocation.', 'No reachability check, MTU probe, TCP connection, or iperf3 process is started.'] },
    Baseline: { description: 'Plan a comparable path collection followed by one throughput sample.', summary: 'The baseline preview composes one path plan and one single-test throughput plan.', steps: ['Plan path evidence for the selected host.', 'Plan one throughput sample against the supplied target; the orchestrator adds its single-test mode.', 'Preserve real output files together only when running from an authorized source checkout.'] },
    WindowsTuning: { description: 'Inspect a Windows tuning verification or dry-run plan before any real state change.', summary: 'The Windows tuning preview never verifies, backs up, applies, or restores local state.', steps: ['Validate the selected action, profile, and managed UDP port.', 'Show the corresponding read-only or dry-run plan.', 'No registry values, QoS policies, NIC properties, power plans, or backup files are touched.'] }
  };
  let active = 'Triage';
  const clean = (value, fallback) => value.trim() || fallback;
  function values() { return { host: clean(q('#host').value, 'example.com'), protocol: q('#protocol').value, round: q('#round').value, skip: q('#skip-pathping').checked, target: clean(q('#iperf-target').value, 'iperf3.example.net'), port: clean(q('#iperf-port').value, '5201'), throughputProtocol: q('#throughput-protocol').value, action: q('#tuning-action').value, profile: q('#tuning-profile').value, udp: clean(q('#udp-port').value, '5201') }; }
  function command() {
    const v = values();
    const hostParameter = v.protocol === 'IPv6' ? 'HostsIPv6' : 'HostsIPv4';
    const path = `pwsh -NoProfile -File .\\Invoke-NetworkLantern.ps1 -Workflow ${active} -${hostParameter} ${v.host} -Protocols ${v.protocol} -Rounds ${v.round}`;
    if (active === 'Path') return `${path}${v.skip ? ' -SkipPathping' : ''} -DryRun`;
    if (active === 'Throughput') return `pwsh -NoProfile -File .\\Invoke-NetworkLantern.ps1 -Workflow Throughput -IperfTarget ${v.target} -IperfPort ${v.port} -ThroughputProtocol ${v.throughputProtocol} -DryRun`;
    if (active === 'WindowsTuning') return `pwsh -NoProfile -File .\\Invoke-NetworkLantern.ps1 -Workflow WindowsTuning -TuningAction ${v.action} -TuningProfile ${v.profile} -UdpPorts ${v.udp} -DryRun`;
    return `${path} -IperfTarget ${v.target} -IperfPort ${v.port} -ThroughputProtocol ${v.throughputProtocol} -DryRun`;
  }
  function render() {
    const state = copy[active];
    q('#workflow-title').textContent = active === 'WindowsTuning' ? 'Windows tuning' : active;
    q('#workflow-description').textContent = state.description;
    q('#plan-summary').textContent = state.summary;
    q('#plan-steps').replaceChildren(...state.steps.map((step) => { const item = document.createElement('li'); item.textContent = step; return item; }));
    q('#command-preview code').textContent = command();
    q('#workflow-panel').setAttribute('aria-labelledby', `tab-${active === 'WindowsTuning' ? 'windows' : active.toLowerCase()}`);
  }
  function choose(workflow) {
    active = workflow;
    tabs.forEach((tab) => tab.setAttribute('aria-selected', String(tab.dataset.workflow === workflow)));
    controls.path.hidden = workflow === 'Throughput' || workflow === 'WindowsTuning';
    controls.throughput.hidden = workflow === 'Path' || workflow === 'WindowsTuning';
    controls.tuning.hidden = workflow !== 'WindowsTuning';
    render();
  }
  tabs.forEach((tab) => tab.addEventListener('click', () => choose(tab.dataset.workflow)));
  tabs.forEach((tab, index) => tab.addEventListener('keydown', (event) => {
    const keys = { ArrowRight: 1, ArrowLeft: -1, Home: -index, End: tabs.length - 1 - index };
    if (!(event.key in keys)) return;
    event.preventDefault();
    const next = tabs[(index + keys[event.key] + tabs.length) % tabs.length];
    next.focus();
    choose(next.dataset.workflow);
  }));
  q('#preview').addEventListener('click', () => { render(); q('#workflow-panel').scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth', block: 'nearest' }); });
  document.querySelectorAll('input, select').forEach((field) => field.addEventListener('change', render));
  render();
})();
