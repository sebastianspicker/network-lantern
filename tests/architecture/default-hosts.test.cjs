const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join, resolve } = require('node:path');
const { test } = require('node:test');
const root = resolve(__dirname, '../..');
const read = path => readFileSync(join(root, path), 'utf8');
const words = text => text.match(/[A-Za-z0-9.-]+/g) ?? [];
const quoted = text => [...text.matchAll(/["']([^"']+)["']/g)].map(match => match[1]);
function required(source, pattern, label) {
  const match = source.match(pattern);
  assert.ok(match, `${label} default host list not found`);
  return match[1];
}
function config(family) {
  return read('config/hosts.conf').split(/\r?\n/)
    .map(line => line.match(new RegExp(`^${family}=(\\S+)\\s*$`)))
    .filter(Boolean).map(match => match[1]);
}
const bash = read('src/bash/path/main.sh');
const powershell = read('src/powershell/path/NetworkLantern.Path/Public/Invoke-NetworkPathDiagnostics.ps1');
const basic = read('crates/path-basic/src/lib.rs');
const trace = read('crates/path-trace/src/lib.rs');
const sources = {
  IPv4: {
    'config/hosts.conf': config('ipv4'),
    'main.sh': words(required(bash, /^PATH_DEFAULT_HOSTS_IPV4=\(([^)]*)\)/m, 'Bash IPv4')),
    'Invoke-NetworkPathDiagnostics.ps1': quoted(required(powershell, /\$defaultHosts4\s*=\s*@\(([^)]*)\)/, 'PowerShell IPv4')),
    'path-basic': quoted(required(basic, /DEFAULT_IPV4_HOSTS:[^=]*=\s*&\[([^\]]*)\]/, 'path-basic IPv4')),
    'path-trace': quoted(required(trace, /DEFAULT_IPV4_HOSTS:[^=]*=\s*&\[([^\]]*)\]/, 'path-trace IPv4')),
  },
  IPv6: {
    'config/hosts.conf': config('ipv6'),
    'main.sh': words(required(bash, /^PATH_DEFAULT_HOSTS_IPV6=\(([^)]*)\)/m, 'Bash IPv6')),
    'Invoke-NetworkPathDiagnostics.ps1': quoted(required(powershell, /\$defaultHosts6\s*=\s*@\(([^)]*)\)/, 'PowerShell IPv6')),
    'path-basic': quoted(required(basic, /DEFAULT_IPV6_HOSTS:[^=]*=\s*&\[([^\]]*)\]/, 'path-basic IPv6')),
    'path-trace': quoted(required(trace, /DEFAULT_IPV6_HOSTS:[^=]*=\s*&\[([^\]]*)\]/, 'path-trace IPv6')),
  },
};
test('default path hosts agree across configuration, Bash, PowerShell and Rust', () => {
  for (const [family, lists] of Object.entries(sources)) {
    const expected = lists['config/hosts.conf'];
    assert.ok(expected.length > 0, `${family} hosts.conf list must not be empty`);
    for (const [name, hosts] of Object.entries(lists)) {
      assert.deepEqual(hosts, expected, `${family} defaults in ${name} differ from config/hosts.conf`);
    }
  }
});
