import { $, control } from './dom';
import { invalidateStatus } from './library';
import { invalidatePreview, syncPathOptions, updateAvailability } from './measurement';
import { flowEntry, isLibraryFlow, visibleFieldsets, type Flow } from './model';
import { invalidateProfileEditor, loadProfiles } from './profiles';
import { invalidateReport, loadRuns } from './reports';
import { state } from './state';

function invalidateLibraryWork() {
  state.libraryRevision++;
  invalidateProfileEditor();
  invalidateReport();
  invalidateStatus('profiles-status');
  invalidateStatus('reports-status');
}

export function navigate(next: Flow) {
  invalidateLibraryWork();
  if (!isLibraryFlow(state.flow)) state.previousFlow = state.flow;
  state.flow = next;
  state.loadedWorkflowEnvelope = false;
  invalidatePreview();
  document.querySelectorAll<HTMLButtonElement>('[data-flow]').forEach(button => {
    button.setAttribute('aria-current', button.dataset.flow === next ? 'page' : 'false');
  });
  $('measurement').hidden = isLibraryFlow(next);
  $('profiles').hidden = next !== 'profiles';
  $('reports').hidden = next !== 'reports';
  const entry = flowEntry(next);
  $('flow-title').textContent = entry.label;
  $('flow-description').textContent = entry.description;
  for (const [id, show] of Object.entries(visibleFieldsets(next))) {
    $(id).hidden = !show;
    $<HTMLFieldSetElement>(id).disabled = !show;
  }
  $('engine-fields').hidden = next !== 'path';
  syncPathOptions();
  control('target').required = false;
  $('plan-summary').replaceChildren();
  $('plan-details').hidden = true;
  $('plan-error').textContent = '';
  updateAvailability();
  if (next === 'profiles') void loadProfiles();
  if (next === 'reports') void loadRuns(0);
}

export function setupNavigation() {
  document.querySelectorAll<HTMLButtonElement>('[data-flow]').forEach(button => {
    button.addEventListener('click', () => navigate(button.dataset.flow as Flow));
  });
}
