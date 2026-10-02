import { escape as e, flows, measurementFlows, flowEntry } from './model';
import { field, select } from './render';

// Formatting whitespace between tags is removed so the rendered DOM contains no layout-affecting text nodes.
const compact = (html: string) => html.replace(/>\s*\n\s*</g, '><').trim();

const hostAttributes = { autocomplete: 'off', spellcheck: 'false', maxlength: '253' };
const portAttributes = { min: '1', max: '65535', required: true } as const;

// The lantern body is drawn in ink; only its flame carries the lamp colour.
const logo = `
  <svg class="mark" viewBox="0 0 32 32" aria-hidden="true">
    <path class="mark-body" d="M11 10h10l2 16H9l2-16Zm2 0V7a3 3 0 0 1 6 0v3M12 13h8M8 26h16"/>
    <path class="mark-flame" d="M16 15.5c-3.4 3.4-2.6 6 0 6s3.4-2.6 0-6Z"/>
  </svg>`;

const header = `
  <a class="skip-link" href="#content">Skip to workspace</a>
  <header class="masthead">
    <h1 class="brand">${logo}<span>Network Lantern</span></h1>
    <div class="runtime">
      <span id="environment">Checking runtime…</span>
      <button id="refresh-runtime" class="quiet compact">Check runtime</button>
    </div>
  </header>`;

const helperPanel = `
  <details class="helper-panel">
    <summary>Privileged helper</summary>
    <p>Needed for native probes and Windows changes. Your operating system asks for approval.</p>
    <p id="helper-detail" class="data" role="status">Checking helper…</p>
    <div class="actions">
      <button id="helper-register" class="secondary compact">Register</button>
      <button id="helper-remove" class="danger compact">Remove</button>
    </div>
  </details>`;

const runStrip = `
  <section id="run-strip" class="run-strip" aria-label="Active measurement" hidden>
    <span class="lamp-indicator" aria-hidden="true"></span>
    <div class="run-head"><strong id="run-title" role="status"></strong><p id="run-count"></p></div>
    <progress id="run-progress"></progress>
    <div class="run-actions">
      <button id="cancel" class="danger">Cancel run</button>
      <button id="open-run" class="secondary" hidden>Open report</button>
    </div>
    <details class="run-log"><summary>Recent activity</summary><pre id="run-logs" tabindex="0"></pre></details>
    <p id="run-error" class="error" role="alert"></p>
  </section>`;

const groups = [
  { id: 'measure', label: 'Measure' },
  { id: 'change', label: 'Change' },
  { id: 'records', label: 'Records' },
] as const;

const navigation = `
  <nav class="rail" aria-label="Workflows">
    ${groups.map(group => `
      <div class="rail-group" role="group" aria-labelledby="rail-${group.id}">
        <p class="rail-label" id="rail-${group.id}">${group.label}</p>
        ${flows.filter(flow => flow.group === group.id).map(({ id, label, description }) => `
          <button data-flow="${e(id)}" aria-current="${id === 'triage' ? 'page' : 'false'}">
            <strong>${e(label)}</strong><span>${e(description)}</span>
          </button>`).join('')}
      </div>`).join('')}
  </nav>`;

const pathFields = `
  <fieldset id="path-fields">
    <legend>Path diagnostics</legend>
    ${field('host', 'Host', '', 'text', { placeholder: 'host.example.net', ...hostAttributes })}
    <div class="two">
      ${select('family', 'Address family', ['IPv4', 'IPv6'])}
      ${select('round', 'Round', ['Standard', 'MTU1400_DF', 'TTL64_Timeout5s'])}
    </div>
    <div id="engine-fields">
      ${select('engine', 'Engine', [
        { value: 'path_basic', label: 'Basic path diagnostics' },
        { value: 'path_trace', label: 'Continuous path trace' },
      ])}
      ${select('trace-type', 'Trace mode', ['ICMP4', 'TCP4', 'UDP4', 'MPLS4', 'AS4'])}
      <p class="hint">AS mode sends hop addresses to Team Cymru DNS to look up their networks.</p>
    </div>
    <label class="check"><input id="skip" type="checkbox">Skip pathping</label>
  </fieldset>`;

const throughputFields = `
  <fieldset id="throughput-fields">
    <legend>Throughput</legend>
    ${field('target', 'Server', '', 'text', { placeholder: 'iperf.example.net', ...hostAttributes })}
    <div class="two">
      ${field('port', 'Port', '5201', 'number', portAttributes)}
      ${select('protocol', 'Protocol', ['Both', 'TCP', 'UDP'])}
    </div>
    ${field('max-tests', 'Maximum tests', '0', 'number', { min: '0', max: '1000000', required: true })}
    <p class="hint">0 means no limit. The default matrix plans up to 1,145 tests, so set a limit on shared links.</p>
  </fieldset>`;

const tuningFields = `
  <fieldset id="tuning-fields" hidden>
    <legend>Windows tuning</legend>
    <div class="two">
      ${select('action', 'Action', ['Verify', 'Backup', 'Apply', 'Restore'])}
      ${select('tuning-profile', 'Profile', ['Safe', 'Measured'])}
    </div>
    ${field('udp-port', 'Managed UDP port', '5201', 'number', portAttributes)}
    ${field('dscp', 'DSCP', '46', 'number', { min: '0', max: '63', required: true })}
    ${field('backup-folder', 'Backup / restore folder', '', 'text', { placeholder: 'Default protected backup location' })}
    ${select('power-plan', 'Power plan', ['None', 'HighPerformance'])}
    <label class="check"><input id="app-policies" type="checkbox">Manage application QoS policies</label>
    <label class="field" for="app-paths">
      <span>Application paths, one per line</span><textarea id="app-paths" rows="3" spellcheck="false"></textarea>
    </label>
    <p class="hint">Apply and Restore change Windows. Keep a backup and an independent way back before you start.</p>
  </fieldset>`;

const advancedSettings = `
  <details class="advanced">
    <summary>Additional settings</summary>
    <label class="field" for="advanced">
      <span>JSON override layer</span><textarea id="advanced" rows="6" spellcheck="false">{}</textarea>
    </label>
    <p class="hint">Applied last, on top of the fields above. Rust validates every key; loaded profiles land here.</p>
    <label class="check"><input id="strict" type="checkbox">Reject unknown settings</label>
  </details>`;

const measurement = `
  <div id="measurement" data-stage="configure">
    <div class="flow-head">
      <h2 id="flow-title">Triage</h2>
      <p id="flow-description" class="lede">${e(flowEntry('triage').summary)}</p>
    </div>
    <div class="controls">
      <form id="configuration">
        ${pathFields}
        ${throughputFields}
        ${tuningFields}
        ${advancedSettings}
        ${field('out', 'Results directory', 'results', 'text', { required: true })}
        <div class="actions">
          <button id="preview" type="submit">Review plan</button>
          <button id="save-config" type="button" class="secondary">Save as profile</button>
        </div>
      </form>
    </div>
    <section class="preview" aria-labelledby="preview-title">
      <div class="preview-head">
        <h2 id="preview-title">Plan</h2>
        <ol class="lifecycle" aria-hidden="true"><li>Configure</li><li>Review</li><li>Run</li></ol>
      </div>
      <p id="capability" class="capability"></p>
      <p id="plan-state" role="status">Configure your workflow and review its plan.</p>
      <div id="plan-error" class="error" role="alert"></div>
      <div id="plan-summary"></div>
      <details id="plan-details" hidden><summary>Resolved plan (JSON)</summary><pre id="plan-json" tabindex="0"></pre></details>
      <div class="start-area">
        <button id="start" class="lamp" disabled>Start reviewed run</button>
        <p class="hint">Starting sends network traffic or performs the selected tuning action.</p>
      </div>
    </section>
  </div>`;

const profileFlowOptions = measurementFlows.map(id => ({ value: id, label: flowEntry(id).label }));

const profiles = `
  <section id="profiles" class="library" hidden>
    <div class="flow-head">
      <h2>Profiles</h2>
      <p class="lede">${e(flowEntry('profiles').summary)}</p>
    </div>
    <div class="source-row">
      ${field('store', 'Profile store', '.iperf3/profiles.json')}
      <button id="load-profiles" class="secondary">Refresh profiles</button>
    </div>
    <p id="profiles-status" class="library-status" role="status"></p>
    <div class="library-grid">
      <div class="index-column"><h3>Saved profiles</h3><ul id="profile-list" class="item-list"></ul></div>
      <form id="profile-form">
        ${field('profile-name', 'Name', '', 'text', { required: true, maxlength: '128' })}
        ${select('profile-flow', 'Load into workflow', profileFlowOptions)}
        <label class="field" for="profile-json">
          <span id="profile-json-label">Parameters (JSON object)</span><textarea id="profile-json" rows="12" spellcheck="false">{}</textarea>
        </label>
        <div class="actions">
          <button type="submit">Save profile</button>
          <button id="use-profile" type="button" class="secondary">Load into workflow</button>
          <button id="delete-profile" type="button" class="danger">Delete</button>
        </div>
        <p id="delete-confirm" class="confirm" hidden><span>Delete this saved profile? This cannot be undone.</span><button type="button" id="confirm-delete" class="danger">Confirm deletion</button>
          <button type="button" id="keep-profile" class="secondary">Keep profile</button>
        </p>
      </form>
    </div>
  </section>`;

const reportDetail = `
  <section id="report-detail" hidden>
    <h3>Selected report</h3>
    <div id="report-metadata"></div>
    <div class="actions">
      <button id="rows-prev" class="secondary compact">Previous measurements</button>
      <button id="rows-next" class="secondary compact">Next measurements</button>
      <span id="rows-page" class="page-label"></span>
    </div>
    <pre id="report-rows" class="record" tabindex="0"></pre>
    <h3>Compare and export</h3>
    <div class="two">
      ${field('baseline-path', 'Baseline report path')}
      ${field('export-path', 'Export destination', '', 'text', { placeholder: 'exports/review.json' })}
    </div>
    <div class="actions">
      <button id="compare" class="secondary">Compare with baseline</button>
      <button id="export" class="secondary">Export JSON</button>
    </div>
    <pre id="comparison" class="record" tabindex="0" hidden></pre>
  </section>`;

const reports = `
  <section id="reports" class="library" hidden>
    <div class="flow-head">
      <h2>Reports</h2>
      <p class="lede">${e(flowEntry('reports').summary)} Missing metrics stay marked unavailable.</p>
    </div>
    <div class="source-row">
      ${field('report-directory', 'Results directory', 'results')}
      <button id="refresh-reports" class="secondary">Refresh runs</button>
    </div>
    <div class="source-row">
      ${field('report-path', 'Report path', '', 'text', { placeholder: 'results/run-id/summary.json' })}
      <button id="read-report" class="secondary">Open report</button>
    </div>
    <p id="reports-status" class="library-status" role="status"></p>
    <div class="table-scroll">
      <table class="ledger">
        <thead><tr><th scope="col">Run</th><th scope="col">Status</th><th scope="col" class="figure">Measurements</th><th scope="col"><span class="visually-hidden">Action</span></th></tr></thead>
        <tbody id="run-list"></tbody>
      </table>
    </div>
    <div class="actions">
      <button id="runs-prev" class="secondary compact">Previous runs</button>
      <button id="runs-next" class="secondary compact">Next runs</button>
      <span id="runs-page" class="page-label"></span>
    </div>
    ${reportDetail}
  </section>`;

export const appMarkup = compact(`
  ${header}
  <div class="frame">
    ${navigation}
    <aside class="system" aria-label="Runtime">
      ${helperPanel}
      <p class="system-note">Runs locally. One measurement at a time. Every run starts from a reviewed plan.</p>
    </aside>
    <main class="desk">
      <div id="runtime-notice" class="notice" role="status"></div>
      ${runStrip}
      <section id="content" tabindex="-1">
        ${measurement}
        ${profiles}
        ${reports}
      </section>
    </main>
  </div>`);
