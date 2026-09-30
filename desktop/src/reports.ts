import { command } from './bridge';
import { $, control } from './dom';
import { beginTask, invalidateStatus } from './library';
import { rowsPageLabel, runsPageLabel, runsStatus, type ReportPage, type RunsPage } from './model';
import { reportMetadata, runList } from './render';
import { state } from './state';

const pageSize = 20;

const reports = {
  runsOffset: 0,
  rowsOffset: 0,
  nextRows: 0,
  // Offsets of earlier measurement pages, so Previous returns exactly where Next came from.
  rowHistory: [] as number[],
  selected: '',
  // Each counter invalidates in-flight responses of its operation.
  reportGeneration: 0,
  runsGeneration: 0,
  comparisonGeneration: 0,
  exportGeneration: 0,
  exportBusy: false,
};

function clearComparison() {
  reports.comparisonGeneration++;
  $('comparison').hidden = true;
  $('comparison').textContent = '';
}

export function invalidateReport() {
  reports.reportGeneration++;
  reports.exportGeneration++;
  reports.selected = '';
  $('report-detail').hidden = true;
  clearComparison();
}

export async function loadRuns(offset: number) {
  const token = ++reports.runsGeneration;
  const navigation = state.libraryRevision;
  const directory = control('report-directory').value;
  const task = beginTask('reports-status', () => (
    token === reports.runsGeneration && navigation === state.libraryRevision && directory === control('report-directory').value
  ));
  await task.run(async () => {
    const data = await command<RunsPage>('runs_list', { directory, offset, limit: pageSize });
    if (!task.current()) return;
    reports.runsOffset = offset;
    $('run-list').innerHTML = runList(data.runs);
    control('runs-prev').disabled = offset === 0;
    control('runs-next').disabled = !data.has_more;
    $('runs-page').textContent = runsPageLabel(offset, data.runs.length, data.total);
    task.message(runsStatus(data));
  });
}

type History = 'reset' | 'next' | 'prev';

export async function readReport(offset: number, path = control('report-path').value, history: History = 'reset') {
  const token = ++reports.reportGeneration;
  const navigation = state.libraryRevision;
  const inputPath = control('report-path').value;
  reports.selected = '';
  $('report-detail').hidden = true;
  clearComparison();
  reports.exportGeneration++;
  const task = beginTask('reports-status', () => (
    token === reports.reportGeneration && navigation === state.libraryRevision && inputPath === control('report-path').value
  ));
  await task.run(async () => {
    const data = await command<ReportPage>('report_read', { path, offset, limit: pageSize });
    if (!task.current()) return;
    if (history === 'next') reports.rowHistory.push(reports.rowsOffset);
    else if (history === 'prev') reports.rowHistory.pop();
    else reports.rowHistory.length = 0;
    reports.selected = path;
    reports.rowsOffset = offset;
    reports.nextRows = data.next_offset;
    $('report-detail').hidden = false;
    $('report-metadata').innerHTML = reportMetadata(path, data.metadata);
    $('report-rows').textContent = JSON.stringify(data.rows, null, 2);
    control('rows-prev').disabled = reports.rowHistory.length === 0;
    control('rows-next').disabled = !data.has_more;
    $('rows-page').textContent = rowsPageLabel(offset, data.next_offset, data.total);
    task.message('Report loaded.');
  });
}

async function compareReport() {
  if (!reports.selected) return;
  const token = ++reports.comparisonGeneration;
  const report = reports.reportGeneration;
  const current = reports.selected;
  const baseline = control('baseline-path').value;
  const task = beginTask('reports-status', () => (
    token === reports.comparisonGeneration && report === reports.reportGeneration
    && current === reports.selected && baseline === control('baseline-path').value
  ));
  await task.run(async () => {
    const data = await command('report_compare', { baseline, current });
    if (!task.current()) return;
    $('comparison').hidden = false;
    $('comparison').textContent = JSON.stringify(data, null, 2);
    task.message('Comparison ready. Null values indicate unavailable evidence.');
  });
}

async function exportReport() {
  if (!reports.selected || reports.exportBusy) return;
  const token = ++reports.exportGeneration;
  const report = reports.reportGeneration;
  const path = reports.selected;
  const destination = control('export-path').value;
  const task = beginTask('reports-status', () => (
    token === reports.exportGeneration && report === reports.reportGeneration
    && path === reports.selected && destination === control('export-path').value
  ));
  reports.exportBusy = true;
  control('export').disabled = true;
  try {
    await task.run(async () => {
      await command('report_export', { path, destination });
      task.message('Report exported.');
    });
  } finally {
    reports.exportBusy = false;
    control('export').disabled = false;
  }
}

export function setupReports() {
  $('report-directory').addEventListener('input', () => {
    reports.runsGeneration++;
    invalidateStatus('reports-status');
    $('run-list').replaceChildren();
    $('runs-page').textContent = '';
    control('runs-prev').disabled = true;
    control('runs-next').disabled = true;
  });
  $('report-path').addEventListener('input', () => {
    invalidateReport();
    invalidateStatus('reports-status');
  });
  $('baseline-path').addEventListener('input', () => {
    clearComparison();
    invalidateStatus('reports-status');
  });
  $('export-path').addEventListener('input', () => {
    reports.exportGeneration++;
    invalidateStatus('reports-status');
  });
  $('refresh-reports').addEventListener('click', () => void loadRuns(0));
  $('runs-prev').addEventListener('click', () => void loadRuns(Math.max(0, reports.runsOffset - pageSize)));
  $('runs-next').addEventListener('click', () => void loadRuns(reports.runsOffset + pageSize));
  $('run-list').addEventListener('click', event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-report]');
    if (button) {
      control('report-path').value = button.dataset.report!;
      void readReport(0);
    }
  });
  $('read-report').addEventListener('click', () => void readReport(0));
  $('rows-next').addEventListener('click', () => void readReport(reports.nextRows, reports.selected, 'next'));
  $('rows-prev').addEventListener('click', () => void readReport(reports.rowHistory.at(-1) || 0, reports.selected, 'prev'));
  $('compare').addEventListener('click', () => void compareReport());
  $('export').addEventListener('click', () => void exportReport());
}
