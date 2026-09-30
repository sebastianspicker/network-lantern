// Controlled local workload only. Baseline files must be saved before editing.
// node scripts/benchmark-tooling.mjs artifacts/audit-benchmark/baseline
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir, platform, release } from 'node:os';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
if (!process.argv[2]) throw new Error('Pass the directory containing baseline runner.sh and Invoke-SecretScan.ps1.');
const baseline = resolve(process.argv[2]);
const fixture = mkdtempSync(join(tmpdir(), 'network-lantern-benchmark-'));
const repetitions = 10;
const run = (command, args, options = {}) => {
  const start = performance.now();
  const result = spawnSync(command, args, { encoding: 'utf8', timeout: 60000, ...options });
  if (result.error) throw result.error;
  return { ...result, elapsedSeconds: (performance.now() - start) / 1000 };
};
const stats = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return { median: (sorted[4] + sorted[5]) / 2, min: sorted[0], max: sorted.at(-1) };
};

try {
  const candidates = join(fixture, 'candidates');
  mkdirSync(candidates);
  const token = 'AK' + 'IA' + 'A'.repeat(16);
  const second = 'gh' + 'p_' + 'b'.repeat(36);
  const contents = ('Sanitized local benchmark text, no target or user information.\n'.repeat(80)) + `${token} ${token} ${second}\n${token.toLowerCase()}\n`;
  for (let index = 0; index < 240; index++) writeFileSync(join(candidates, `${index}.txt`), contents);
  writeFileSync(join(candidates, 'utf16.txt'), Buffer.concat([Buffer.from([255, 254]), Buffer.from(`${second}\n`, 'utf16le')]));
  const captureHarness = join(fixture, 'capture.sh');
  writeFileSync(captureHarness, `source "$1"
CURRENT_TMP="$2/output"
TABLE_LOG="$2/errors"
MTR_TIMEOUT_SECONDS=20
POLL_LOG="$2/polls"
: > "$POLL_LOG"
sleep() { printf '%s\\n' "$1" >> "$POLL_LOG"; command sleep "$1"; }
mtr() { command sleep 5; printf '{}\\n'; }
_capture_mtr_with_deadline fixture.example
status=$?
[[ $(<"$CURRENT_TMP") == '{}' ]] || exit 99
printf '%s\\n' "$status"
exit "$status"
`);
  const samples = { baseline: { polling: [], scanning: [] }, current: { polling: [], scanning: [] } };
  const variants = {
    baseline: { runner: join(baseline, 'runner.sh'), scanner: join(baseline, 'Invoke-SecretScan.ps1') },
    current: { runner: join(root, 'src/bash/path/lib/runner.sh'), scanner: join(root, 'scripts/Invoke-SecretScan.ps1') },
  };
  // One warm-up per variant followed by ten measured repetitions. Alternate
  // order to reduce systematic drift; use the exact same immutable fixture.
  for (let iteration = -1; iteration < repetitions; iteration++) {
    const order = iteration % 2 === 0 ? ['current', 'baseline'] : ['baseline', 'current'];
    for (const name of order) {
      const variant = variants[name];
      const polling = run('bash', [captureHarness, variant.runner, fixture]);
      assert.equal(polling.status, 0, polling.stderr);
      assert.equal(polling.stdout.trim(), '0');
      const sleeps = readFileSync(join(fixture, 'polls'), 'utf8').trim().split('\n').length;
      const scanning = run('pwsh', ['-NoProfile', '-NonInteractive', '-File', variant.scanner, '-Path', candidates], { env: { ...process.env, NO_COLOR: '1', TERM: 'dumb' } });
      assert.equal(scanning.status, 1);
      const count = Number(scanning.stderr.match(/Potential secrets detected \((\d+)\)/)?.[1]);
      assert.equal(count, 721, scanning.stderr);
      assert.ok(!scanning.stderr.includes(token), 'Scanner exposed matched text');
      if (iteration >= 0) {
        // Shell + fake MTR subshell + its one sleep + polling sleep processes.
        samples[name].polling.push({ elapsedSeconds: polling.elapsedSeconds, sleeps, processCount: sleeps + 3, status: polling.status });
        samples[name].scanning.push({ elapsedSeconds: scanning.elapsedSeconds, processCount: 1, matches: count });
      }
    }
    process.stderr.write(`${iteration < 0 ? 'Warm-up' : `Repetition ${iteration + 1}`} complete\n`);
  }
  const summary = Object.fromEntries(Object.entries(samples).map(([name, sample]) => [name, {
    pollingSeconds: stats(sample.polling.map((s) => s.elapsedSeconds)),
    pollingSleeps: stats(sample.polling.map((s) => s.sleeps)),
    pollingProcesses: stats(sample.polling.map((s) => s.processCount)),
    scanningSeconds: stats(sample.scanning.map((s) => s.elapsedSeconds)),
    scanningProcesses: 1,
    candidateFileTraversals: 241 * (name === 'baseline' ? 18 : 1),
    matches: 721,
  }]));
  assert.ok(summary.current.pollingSleeps.median <= summary.baseline.pollingSleeps.median * 0.25);
  assert.ok(summary.current.pollingSeconds.median <= summary.baseline.pollingSeconds.median + 0.65);
  const report = {
    environment: { platform: platform(), release: release(), node: process.version,
      bash: run('bash', ['--version']).stdout.split('\n')[0],
      powershell: run('pwsh', ['-NoProfile', '-NonInteractive', '-Command', '$PSVersionTable.PSVersion.ToString()']).stdout.trim() },
    workload: { candidates: 241, bytes: 240 * Buffer.byteLength(contents) + 2 + Buffer.byteLength(`${second}\n`, 'utf16le'), warmups: 1, repetitions, controlledProcessSeconds: 5 },
    summary, samples,
  };
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
