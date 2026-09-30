import { escape as e, flows, measurementFlows, flowEntry } from './model';
import { field, select } from './render';

// Formatting whitespace between tags is removed so the rendered DOM contains no layout-affecting text nodes.
const compact = (html: string) => html.replace(/>\s*\n\s*</g, '><').trim();

const hostAttributes = { autocomplete: 'off', spellcheck: 'false', maxlength: '253' };
const portAttributes = { min: '1', max: '65535', required: true } as const;

const logo = `
  <svg viewBox="0 0 32 32" aria-hidden="true">
    <circle cx="16" cy="16" r="15"/>
    <path d="M11 10h10l2 16H9l2-16Zm2 0V7a3 3 0 0 1 6 0v3M12 13h8M16 15c-4 4-3 7 0 7s4-3 0-7Z"/>
  </svg>`;

const header = `
  <a class="skip-link" href="#content">Skip to workspace</a>
  <header>
    <div class="brand">${logo}Network Lantern</div>
    <span id="environment">Checking runtime…</span>
    <button id="refresh-runtime" class="secondary">Check runtime</button>
  </header>`;

const helperPanel = `
  <details class="helper-panel">
    <summary>Privileged helper</summary>
    <p>Register the helper when native probes or Windows changes require authorization. The operating system controls approval.</p>
    <p id="helper-detail" role="status">Checking helper…</p>
    <div class="actions">
      <button id="helper-register" class="secondary">Register helper</button>
      <button id="helper-remove" class="danger">Remove helper</button>
    </div>
  </details>`;

const runStrip = `
  <section id="run-strip" class="run-strip" aria-label="Active measurement" hidden>
    <div><strong id="run-title" role="status"></strong><p id="run-count"></p></div>
    <progress id="run-progress"></progress>
    <button id="cancel" class="danger">Cancel run</button>
    <button id="open-run" class="secondary" hidden>Open report</button>
    <details><summary>Recent activity</summary><pre id="run-logs" tabindex="0"></pre></details>
    <p id="run-error" class="error" role="alert"></p>
  </section>`;

const navigation = `
  <nav aria-label="Workflows">
    ${flows.map(({ id, label, description }) => `
      <button data-flow="${e(id)}" aria-current="${id === 'triage' ? 'page' : 'false'}">
        <strong>${e(label)}</strong><span>${e(description)}</span>
      </button>`).join('')}
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
      <p class="hint">AS4/AS6 discloses hop addresses to Team Cymru DNS.</p>
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
    <p class="hint">0 means unlimited. The Rust preview checks the full matrix against this limit.</p>
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
    <p class="hint">Review platform support and recovery requirements before any change.</p>
  </fieldset>`;

const advancedSettings = `
  <details class="advanced">
    <summary>Additional settings</summary>
    <label class="field" for="advanced">
      <span>JSON override layer</span><textarea id="advanced" rows="6" spellcheck="false">{}</textarea>
    </label>
    <p class="hint">Rust validates keys and values. Use this layer for detailed engine settings or loaded profiles.</p>
    <label class="check"><input id="strict" type="checkbox">Reject unknown settings</label>
  </details>`;

const measurement = `
  <div id="measurement">
    <div class="controls">
      <h2 id="flow-title">Triage</h2>
      <p id="flow-description" class="muted">Path and throughput together</p>
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
      <h2 id="preview-title">Your preview</h2>
      <p class="muted">Review targets, scope, and permissions before starting.</p>
      <div id="capability" class="notice"></div>
      <p id="plan-state" role="status">Configure your workflow and review its plan.</p>
      <div id="plan-error" class="error" role="alert"></div>
      <div id="plan-summary"></div>
      <details id="plan-details" hidden><summary>Resolved plan</summary><pre id="plan-json" tabindex="0"></pre></details>
      <div class="start-area">
        <button id="start" disabled>Start reviewed run</button>
        <p class="hint">Starting sends network traffic or performs the selected tuning action.</p>
      </div>
    </section>
  </div>`;

const profileFlowOptions = measurementFlows.map(id => ({ value: id, label: flowEntry(id).label }));

const profiles = `
  <section id="profiles" class="library" hidden>
    <h2>Profiles</h2>
    <p class="muted">Store reusable settings locally. Loaded profiles still require a fresh plan.</p>
    ${field('store', 'Profile store', '.iperf3/profiles.json')}
    <button id="load-profiles" class="secondary">Refresh profiles</button>
    <p id="profiles-status" role="status"></p>
    <div class="library-grid">
      <div><h3>Saved profiles</h3><ul id="profile-list" class="item-list"></ul></div>
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
        <p id="delete-confirm" hidden>Delete this saved profile? <button type="button" id="confirm-delete" class="danger">Confirm deletion</button>
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
      <button id="rows-prev" class="secondary">Previous measurements</button>
      <button id="rows-next" class="secondary">Next measurements</button>
      <span id="rows-page"></span>
    </div>
    <pre id="report-rows" tabindex="0"></pre>
    <div class="two">
      ${field('baseline-path', 'Baseline report path')}
      ${field('export-path', 'Export destination', '', 'text', { placeholder: 'exports/review.json' })}
    </div>
    <div class="actions">
      <button id="compare" class="secondary">Compare with baseline</button>
      <button id="export" class="secondary">Export JSON</button>
    </div>
    <pre id="comparison" tabindex="0" hidden></pre>
  </section>`;

const reports = `
  <section id="reports" class="library" hidden>
    <h2>Reports</h2>
    <p class="muted">Inspect recorded results and compare runs. Missing metrics remain unavailable.</p>
    <div class="two">
      ${field('report-directory', 'Results directory', 'results')}
      ${field('report-path', 'Report path', '', 'text', { placeholder: 'results/run-id/summary.json' })}
    </div>
    <div class="actions">
      <button id="refresh-reports" class="secondary">Refresh runs</button>
      <button id="read-report" class="secondary">Open report</button>
    </div>
    <p id="reports-status" role="status"></p>
    <div class="table-scroll">
      <table>
        <thead><tr><th>Run</th><th>Status</th><th>Measurements</th><th></th></tr></thead>
        <tbody id="run-list"></tbody>
      </table>
    </div>
    <div class="actions">
      <button id="runs-prev" class="secondary">Previous runs</button>
      <button id="runs-next" class="secondary">Next runs</button>
      <span id="runs-page"></span>
    </div>
    ${reportDetail}
  </section>`;

export const appMarkup = compact(`
  ${header}
  <main>
    <div class="intro"><h1>Investigate your network.</h1><p>Configure a workflow. Review its plan. Keep the evidence.</p></div>
    <div id="runtime-notice" class="notice" role="status"></div>
    ${helperPanel}
    ${runStrip}
    <div class="workbench">
      ${navigation}
      <section id="content" tabindex="-1">
        ${measurement}
        ${profiles}
        ${reports}
      </section>
    </div>
    <footer>Local Rust runtime · One active measurement · Review before execution</footer>
  </main>`);
