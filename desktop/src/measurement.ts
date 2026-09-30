import { native } from './bridge';
import { $, control } from './dom';
import {
  active, capabilityFor, capabilityNotice, disabledControls, fingerprint, nonEmptyLines, pathRounds, traceTypeFor, traceTypes,
  workflowRequest, type Inputs, type Request,
} from './model';
import { options } from './render';
import { state } from './state';

export function inputs(): Inputs {
  return {
    host: control('host').value.trim(),
    family: control('family').value,
    round: control('round').value,
    engine: control('engine').value,
    traceType: control('trace-type').value,
    skip: control('skip').checked,
    target: control('target').value,
    port: Number(control('port').value),
    protocol: control('protocol').value,
    maxTests: Number(control('max-tests').value),
    action: control('action').value,
    profile: control('tuning-profile').value,
    udpPort: Number(control('udp-port').value),
    advanced: control('advanced').value,
    strict: control('strict').checked,
    backupFolder: control('backup-folder').value,
    dscp: Number(control('dscp').value),
    powerPlan: control('power-plan').value,
    includeAppPolicies: control('app-policies').checked,
    appPaths: nonEmptyLines(control('app-paths').value),
  };
}

export function currentRequest(): Request {
  return workflowRequest(state.flow, inputs(), state.loadedWorkflowEnvelope);
}

export function invalidatePreview() {
  $('plan-summary').replaceChildren();
  $('plan-details').hidden = true;
  state.generation++;
  state.previewKey = '';
  state.previewRequest = null;
  $('start').setAttribute('disabled', '');
  $('plan-state').textContent = 'Settings changed. Review a fresh plan before starting.';
}

function previewIsFresh(): boolean {
  try {
    return state.previewKey === fingerprint(currentRequest(), control('out').value) && !!state.previewKey;
  } catch {
    return false; // Invalid JSON awaits Rust review.
  }
}

export function updateAvailability() {
  const capability = capabilityFor(state.doctor, state.flow, control('engine').value);
  $('capability').textContent = capabilityNotice(capability);
  const disabled = disabledControls({
    native,
    available: capability.available,
    fresh: previewIsFresh(),
    running: active(state.run),
    starting: state.starting,
    planning: state.planning,
    helperBusy: state.helperBusy,
  });
  control('start').disabled = disabled.start;
  control('preview').disabled = disabled.preview;
  control('helper-register').disabled = disabled.helperRegister;
  control('helper-remove').disabled = disabled.helperRemove;
}

export function syncPathOptions() {
  const trace = state.flow === 'path' && control('engine').value === 'path_trace';
  const rounds = pathRounds(trace);
  const priorRound = control('round').value;
  $('round').innerHTML = options(rounds);
  control('round').value = rounds.includes(priorRound) ? priorRound : 'Standard';
  const family = control('family').value;
  const traceType = traceTypeFor(control('trace-type').value, family);
  $('trace-type').innerHTML = options(traceTypes(family));
  control('trace-type').value = traceType;
  control('trace-type').disabled = !trace;
}
