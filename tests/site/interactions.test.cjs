const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { test } = require('node:test');
const { createContext, runInContext } = require('node:vm');
const site = resolve(__dirname, '../../site');
const html = readFileSync(resolve(site, 'index.html'), 'utf8');

function element(id = '') {
  return { id, value: '', checked: false, hidden: false, attributes: {}, events: {}, dataset: {},
    setAttribute(name, value) { this.attributes[name] = value; },
    addEventListener(name, callback) { this.events[name] = callback; },
    replaceChildren(...children) { this.children = children; },
    append(...children) { this.children = children; },
    focus() { this.focused = true; }, scrollIntoView() {},
  };
}

function planner(clipboard = { writeText: async () => {} }) {
  const nodes = new Map([...html.matchAll(/\bid="([^"]+)"/g)].map(([, id]) => [`#${id}`, element(id)]));
  nodes.set('#command-preview code', element());
  nodes.set('.workflow-tabs', element());
  for (const [tag] of html.matchAll(/<input\b[^>]+>/g)) {
    const node = nodes.get(`#${tag.match(/\bid="([^"]+)"/)[1]}`);
    node.value = tag.match(/\bvalue="([^"]*)"/)?.[1] ?? '';
  }
  for (const [, id, content] of html.matchAll(/<select\b[^>]*id="([^"]+)"[^>]*>(.*?)<\/select>/g)) {
    nodes.get(`#${id}`).value = content.match(/<option>(.*?)<\/option>/)[1];
  }
  const tabs = [...html.matchAll(/<button\b[^>]*data-workflow="([^"]+)"[^>]*>/g)].map(([tag, workflow]) => {
    const node = nodes.get(`#${tag.match(/\bid="([^"]+)"/)[1]}`);
    node.dataset.workflow = workflow;
    return node;
  });
  const context = createContext({
    document: { querySelector: (selector) => nodes.get(selector) ?? null,
      querySelectorAll: (selector) => { assert.equal(selector, '[role="tab"]'); return tabs; },
      createElement: () => element(), createRange: () => ({ selectNodeContents() {} }),
    },
    window: { getSelection: () => ({ removeAllRanges() {}, addRange() {} }) },
    navigator: { clipboard }, URL, queueMicrotask,
    matchMedia: () => ({ matches: false, addEventListener() {} }),
  });
  for (const file of ['planner.js', 'app.js']) runInContext(readFileSync(resolve(site, file), 'utf8'), context);
  return { get: (id) => nodes.get(`#${id}`), tabs, edit(id, value) { nodes.get(`#${id}`).value = value; nodes.get('#configuration').events.input(); } };
}

test('keyboard navigation maintains one tab stop and the matching tab panel', () => {
  const app = planner();
  app.tabs[0].events.keydown({ key: 'End', preventDefault() {} });
  assert.equal(app.get('workflow-title').textContent, 'Windows tuning');
  assert.equal(app.get('workflow-panel').attributes['aria-labelledby'], 'tab-windows');
  assert.equal(app.tabs.at(-1).focused, true);
  assert.equal(app.tabs.filter((tab) => tab.tabIndex === 0).length, 1);
  app.tabs.at(-1).events.keydown({ key: 'ArrowDown', preventDefault() {} });
  assert.equal(app.get('workflow-title').textContent, 'Triage');
  app.tabs[1].events.click();
  assert.equal(app.get('throughput-controls').hidden, true);
  assert.equal(app.get('path-controls').hidden, false);
});

test('invalid input disables copy, identifies its field, then clears on correction', () => {
  const app = planner();
  app.edit('iperf-port', '99999');
  assert.equal(app.get('copy-command').disabled, true);
  assert.equal(app.get('iperf-port').attributes['aria-invalid'], 'true');
  assert.equal(app.get('iperf-port-error').hidden, false);
  assert.match(app.get('validation-summary').textContent, /highlighted field/);
  app.edit('iperf-port', '5202');
  assert.equal(app.get('copy-command').disabled, false);
  assert.equal(app.get('iperf-port-error').hidden, true);
  assert.match(app.get('command-preview code').textContent, /"port":5202/);
});

test('copy reports success only after writing the current dry-run command', async () => {
  let copied;
  const app = planner({ writeText: async (text) => { copied = text; } });
  await app.get('copy-command').events.click();
  assert.equal(copied, app.get('command-preview code').textContent);
  assert.match(copied, /--dry-run$/);
  assert.equal(app.get('copy-label').textContent, 'Copied');
  app.edit('host', 'router.example');
  assert.equal(app.get('copy-label').textContent, 'Copy command');
  assert.equal(app.get('copy-status').textContent, '');
});

test('clipboard failure offers manual copying without claiming success', async () => {
  const app = planner({ writeText: async () => { throw new Error('permission denied'); } });
  await app.get('copy-command').events.click();
  assert.match(app.get('copy-status').textContent, /copy it manually/);
  assert.notEqual(app.get('copy-label').textContent, 'Copied');
  assert.equal(app.get('command-preview').focused, true);
});

test('reset restores hidden and active settings while preserving the selected workflow', () => {
  const app = planner();
  app.edit('host', 'router.example');
  app.edit('max-tests', '12');
  app.get('skip-pathping').checked = true;
  app.tabs[2].events.click();
  app.get('reset').events.click();
  assert.equal(app.get('workflow-title').textContent, 'Throughput');
  assert.equal(app.get('max-tests').value, '0');
  assert.equal(app.get('host').value, 'example.com');
  assert.equal(app.get('skip-pathping').checked, false);
  assert.equal(app.get('copy-command').disabled, false);
});

test('a pending clipboard write identifies stale settings when it completes', async () => {
  let complete;
  const app = planner({ writeText: () => new Promise((resolve) => { complete = resolve; }) });
  const pending = app.get('copy-command').events.click();
  assert.equal(app.get('copy-command').disabled, true);
  app.edit('iperf-port', '5202');
  complete();
  await pending;
  assert.match(app.get('copy-status').textContent, /previous command was copied/);
  assert.notEqual(app.get('copy-label').textContent, 'Copied');
  assert.equal(app.get('copy-command').disabled, false);
});
