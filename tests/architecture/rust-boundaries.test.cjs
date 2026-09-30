const assert = require('node:assert/strict');
const { readFileSync, readdirSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { test } = require('node:test');
const root = resolve(__dirname, '../..');
function files(path) {
  return readdirSync(path, { withFileTypes: true }).flatMap(entry => entry.isDirectory() ? files(join(path, entry.name)) : [join(path, entry.name)]);
}
test('measurement engines do not spawn command-line measurement tools', () => {
  for (const crate of ['packet', 'path-io', 'path-basic', 'path-trace', 'throughput']) {
    for (const path of files(join(root, 'crates', crate, 'src')).filter(path => path.endsWith('.rs'))) {
      const source = readFileSync(path, 'utf8').split('#[cfg(test)]')[0];
      assert.doesNotMatch(source, /(?:std|tokio)::process|Command::new|\.ps1["']|\.sh["']/, path);
    }
  }
});
test('packet parsing remains independent of IO and capability plans', () => {
  const manifest = readFileSync(join(root, 'crates/packet/Cargo.toml'), 'utf8');
  assert.doesNotMatch(manifest, /lantern-(path|throughput|runtime)|tokio|socket2|libc/);
});
test('static planner has no persistent or native execution APIs', () => {
  for (const file of ['site/app.js', 'site/planner.js']) {
    assert.doesNotMatch(readFileSync(join(root, file), 'utf8'), /\b(?:localStorage|sessionStorage|indexedDB|fetch|XMLHttpRequest|WebSocket|__TAURI__)\b/);
  }
});
test('production desktop does not enable test permissions or remote code', () => {
  const config = JSON.parse(readFileSync(join(root, 'desktop/src-tauri/tauri.conf.json'), 'utf8'));
  assert.equal(config.app.withGlobalTauri, undefined);
  assert.doesNotMatch(JSON.stringify(config.app.security), /wdio|unsafe-eval|https:/);
  const capability = JSON.parse(readFileSync(join(root, 'desktop/src-tauri/capabilities/main.json'), 'utf8'));
  assert.deepEqual(capability.permissions, ['core:default']);
  const manifest = readFileSync(join(root, 'desktop/src-tauri/Cargo.toml'), 'utf8');
  assert.match(manifest, /default = \["custom-protocol"\]/);
  assert.match(manifest, /custom-protocol = \["tauri\/custom-protocol"\]/);
  assert.match(manifest, /tauri-plugin-wdio = \{[^\n]+optional = true/);
});
test('workspace crates depend only along the documented direction', () => {
  // Keep in sync with the crate graph in docs/architecture.md. Cargo rejects cycles but
  // not a wrong direction, so every internal path dependency is listed explicitly.
  const allowed = {
    'crates/contracts': [],
    'crates/packet': [],
    'crates/tuning': [],
    'crates/platform': ['contracts'],
    'crates/path-io': ['packet'],
    'crates/path-basic': ['packet', 'path-io'],
    'crates/path-trace': ['packet', 'path-io'],
    'crates/throughput': ['contracts', 'packet', 'path-io'],
    'crates/helper': ['contracts', 'path-basic', 'path-io', 'path-trace', 'throughput', 'tuning'],
    'crates/runtime': ['contracts', 'helper', 'packet', 'path-basic', 'path-io', 'path-trace', 'platform', 'throughput', 'tuning'],
    'crates/cli': ['contracts', 'platform', 'runtime'],
    'desktop/src-tauri': ['contracts', 'runtime'],
  };
  const workspace = readFileSync(join(root, 'Cargo.toml'), 'utf8').match(/^members\s*=\s*\[([^\]]*)\]/m);
  assert.ok(workspace, 'workspace members not found');
  const crates = [...workspace[1].matchAll(/"([^"]+)"/g)].map(match => match[1]);
  assert.deepEqual(crates.slice().sort(), Object.keys(allowed).sort());
  for (const crate of crates) {
    const manifest = readFileSync(join(root, crate, 'Cargo.toml'), 'utf8');
    const edges = [...manifest.matchAll(/^lantern-([a-z-]+)\s*=\s*\{[^}]*\bpath\s*=/gm)].map(match => match[1]).sort();
    assert.deepEqual(edges, allowed[crate], crate);
  }
});
