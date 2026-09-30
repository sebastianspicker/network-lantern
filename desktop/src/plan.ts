import { command } from './bridge';
import { $, control } from './dom';
import { currentRequest, invalidatePreview, syncPathOptions, updateAvailability } from './measurement';
import { active, errorMessage, fingerprint, type Json, type Request } from './model';
import { planSummary } from './render';
import { pollRun } from './run';
import { state } from './state';

async function reviewPlan(event: Event) {
  event.preventDefault();
  const token = ++state.generation;
  state.planning = true;
  $('plan-error').textContent = '';
  $('plan-state').textContent = 'Resolving and validating the plan…';
  updateAvailability();
  try {
    const request = currentRequest();
    const key = fingerprint(request, control('out').value);
    const result = await command<Json>('plan', { request });
    if (token !== state.generation) return;
    state.previewRequest = (result.resolved_request as unknown as Request | undefined) ?? request;
    state.previewKey = key;
    $('plan-state').textContent = 'Plan ready. Review the resolved settings before starting.';
    $('plan-json').textContent = JSON.stringify(result, null, 2);
    $('plan-details').hidden = false;
    $<HTMLDetailsElement>('plan-details').open = true;
    $('plan-summary').innerHTML = planSummary(result);
  } catch (error) {
    if (token === state.generation) {
      state.previewKey = '';
      state.previewRequest = null;
      $('plan-error').textContent = errorMessage(error);
      $('plan-state').textContent = 'Plan unavailable. Resolve the issue and review again.';
    }
  } finally {
    state.planning = false;
    updateAvailability();
  }
}

async function startRun() {
  const stale = !state.previewRequest || !state.previewKey || state.previewKey !== fingerprint(currentRequest(), control('out').value);
  if (stale || active(state.run) || state.starting || state.helperBusy) return;
  state.starting = true;
  updateAvailability();
  $('plan-error').textContent = '';
  try {
    await command('start_run', { request: state.previewRequest, out: control('out').value });
    invalidatePreview();
    await pollRun();
  } catch (error) {
    $('plan-error').textContent = errorMessage(error);
  } finally {
    state.starting = false;
    updateAvailability();
  }
}

export function setupPlan() {
  const settingsChanged = () => {
    invalidatePreview();
    updateAvailability();
  };
  $('engine').addEventListener('change', syncPathOptions);
  $('family').addEventListener('change', syncPathOptions);
  $('configuration').addEventListener('input', settingsChanged);
  $('configuration').addEventListener('change', settingsChanged);
  $('configuration').addEventListener('submit', event => void reviewPlan(event));
  $('start').addEventListener('click', () => void startRun());
}
