import { command, native } from './bridge';
import { $, control } from './dom';
import { updateAvailability } from './measurement';
import { active, errorMessage, runSummary, type Progress } from './model';
import { navigate } from './navigation';
import { readReport } from './reports';
import { state } from './state';

const pollInterval = 250;
// A successful poll clears only an error the poller wrote, so a cancel failure stays visible.
let pollErrorShown = false;

function renderRun() {
  const run = state.run;
  $('run-strip').hidden = !run;
  if (!run) return;
  $('run-strip').dataset.state = run.state;
  const summary = runSummary(run);
  if ($('run-title').textContent !== summary.title) $('run-title').textContent = summary.title;
  $('run-count').textContent = summary.count;
  const progress = $<HTMLProgressElement>('run-progress');
  progress.max = Math.max(run.total, 1);
  progress.value = run.completed;
  // Rewriting unchanged text every poll would clear the user's selection.
  if ($('run-logs').textContent !== summary.logs) $('run-logs').textContent = summary.logs;
  control('cancel').disabled = run.state !== 'running';
  $('cancel').hidden = !active(run);
  $('open-run').hidden = !run.report_path;
}

// Polls never overlap; a poll requested while one is in flight is skipped.
export async function pollRun() {
  if (!native || state.polling) return;
  state.polling = true;
  try {
    state.run = await command<Progress | null>('run_status');
    renderRun();
    if (pollErrorShown) {
      $('run-error').textContent = '';
      pollErrorShown = false;
    }
  } catch (error) {
    $('run-error').textContent = errorMessage(error);
    pollErrorShown = true;
  } finally {
    state.polling = false;
    updateAvailability();
  }
}

export function startPolling() {
  void pollRun();
  setInterval(() => void pollRun(), pollInterval);
}

export function setupRun() {
  $('cancel').addEventListener('click', async () => {
    if (!state.run) return;
    control('cancel').disabled = true;
    try {
      $('run-error').textContent = '';
      pollErrorShown = false;
      await command('cancel_run', { runId: state.run.run_id });
      await pollRun();
    } catch (error) {
      $('run-error').textContent = errorMessage(error);
      pollErrorShown = false;
      control('cancel').disabled = false;
    }
  });
  $('open-run').addEventListener('click', () => {
    const report = state.run?.report_path;
    if (!report) return;
    navigate('reports');
    control('report-path').value = report;
    void readReport(0);
  });
}
