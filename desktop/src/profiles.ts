import { command } from './bridge';
import { $, control } from './dom';
import { beginTask, invalidateStatus } from './library';
import { currentRequest } from './measurement';
import { errorMessage, parseParameters, profileDestination, type Flow, type Json, type Request } from './model';
import { profileList } from './render';
import { state } from './state';

interface DeleteTarget {
  store: string;
  name: string;
  revision: number;
}

const profiles = {
  // Incremented per list request and store edit so only the latest list is shown.
  generation: 0,
  // Incremented whenever the editor draft changes so pending reads and saves cannot overwrite it.
  revision: 0,
  loading: false,
  mutationBusy: false,
  deleteTarget: null as DeleteTarget | null,
  // A captured workflow request awaiting its first save; Rust resolves it into reusable parameters.
  pendingRequest: null as Request | null,
};

function updateProfileControls() {
  const locked = profiles.mutationBusy || profiles.loading;
  $<HTMLFormElement>('profile-form').querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled = locked;
  control('delete-profile').disabled = locked;
  control('confirm-delete').disabled = locked;
  control('use-profile').disabled = profiles.loading;
}

export function invalidateProfileEditor() {
  profiles.revision++;
  profiles.loading = false;
  profiles.deleteTarget = null;
  $('delete-confirm').hidden = true;
  updateProfileControls();
}

function showProfileParameters(data: Json) {
  profiles.pendingRequest = null;
  $<HTMLTextAreaElement>('profile-json').readOnly = false;
  $('profile-json-label').textContent = 'Parameters (JSON object)';
  control('profile-json').value = JSON.stringify(data, null, 2);
}

function editorIsCurrent(revision: number, navigation: number, store: string) {
  return () => revision === profiles.revision && navigation === state.libraryRevision && store === control('store').value;
}

export async function loadProfiles(announce = true) {
  const token = ++profiles.generation;
  const navigation = state.libraryRevision;
  const store = control('store').value;
  const current = () => token === profiles.generation && navigation === state.libraryRevision && store === control('store').value;
  const task = announce ? beginTask('profiles-status', current) : null;
  try {
    const names = await command<string[]>('profiles_list', { store });
    if (!current()) return;
    $('profile-list').innerHTML = profileList(names);
    task?.message(names.length ? `${names.length} saved profiles` : 'No profiles in this store. Save a configuration to get started.');
  } catch (error) {
    task?.message(errorMessage(error));
  }
}

async function readProfile(name: string) {
  invalidateProfileEditor();
  const store = control('store').value;
  const current = editorIsCurrent(profiles.revision, state.libraryRevision, store);
  profiles.loading = true;
  updateProfileControls();
  const task = beginTask('profiles-status', current);
  await task.run(async () => {
    const data = await command<Json>('profiles_get', { store, name });
    if (!task.current()) return;
    control('profile-name').value = name;
    showProfileParameters(data);
    task.message('Profile loaded for review.');
  });
  if (task.current()) {
    profiles.loading = false;
    updateProfileControls();
  }
}

async function saveProfile() {
  if (profiles.mutationBusy || profiles.loading) return;
  const store = control('store').value;
  const name = control('profile-name').value;
  const request = profiles.pendingRequest;
  const navigation = state.libraryRevision;
  const current = editorIsCurrent(profiles.revision, navigation, store);
  const task = beginTask('profiles-status', current);
  profiles.mutationBusy = true;
  updateProfileControls();
  await task.run(async () => {
    // Capture all write inputs before yielding; edits affect the next save only.
    const parameters = request ? null : parseParameters(control('profile-json').value);
    if (request) {
      await command('profiles_save_request', { store, name, request });
      if (current()) {
        const saved = await command<Json>('profiles_get', { store, name });
        if (current()) showProfileParameters(saved);
      }
    } else {
      await command('profiles_save', { store, name, parameters });
    }
    task.message(`Saved profile "${name}".`);
    if (navigation === state.libraryRevision && store === control('store').value) await loadProfiles(false);
  });
  profiles.mutationBusy = false;
  updateProfileControls();
}

async function deleteProfile() {
  const target = profiles.deleteTarget;
  if (!target || profiles.mutationBusy || profiles.loading) return;
  if (target.revision !== profiles.revision || target.store !== control('store').value || target.name !== control('profile-name').value) return;
  const navigation = state.libraryRevision;
  const task = beginTask('profiles-status', editorIsCurrent(target.revision, navigation, target.store));
  profiles.deleteTarget = null;
  $('delete-confirm').hidden = true;
  profiles.mutationBusy = true;
  updateProfileControls();
  await task.run(async () => {
    await command('profiles_delete', { store: target.store, name: target.name });
    task.message(`Deleted profile "${target.name}".`);
    if (navigation === state.libraryRevision && target.store === control('store').value) await loadProfiles(false);
  });
  profiles.mutationBusy = false;
  updateProfileControls();
}

function saveConfiguration(navigate: (flow: Flow) => void) {
  try {
    const request = currentRequest();
    profiles.pendingRequest = request;
    control('profile-json').value = JSON.stringify(request, null, 2);
    $<HTMLTextAreaElement>('profile-json').readOnly = true;
    $('profile-json-label').textContent = 'Request to resolve in Rust (read-only)';
    navigate('profiles');
    control('profile-flow').value = state.previousFlow;
    control('profile-name').focus();
  } catch (error) {
    $('plan-error').textContent = errorMessage(error);
  }
}

function useProfile(navigate: (flow: Flow) => void) {
  try {
    if (profiles.pendingRequest) throw new Error('Save this configuration first so Rust can resolve its reusable parameters.');
    const data = parseParameters(control('profile-json').value);
    control('advanced').value = JSON.stringify(data, null, 2);
    const destination = profileDestination(data, control('profile-flow').value as Flow);
    if (destination.engine) control('engine').value = destination.engine;
    navigate(destination.flow);
    state.loadedWorkflowEnvelope = data.capability === 'workflow';
    $('plan-state').textContent = 'Profile loaded as the final settings layer. Edit Additional settings and review a fresh plan before starting.';
  } catch (error) {
    $('profiles-status').textContent = errorMessage(error);
  }
}

export function setupProfiles(navigate: (flow: Flow) => void) {
  const editorChanged = () => {
    invalidateProfileEditor();
    invalidateStatus('profiles-status');
  };
  $('load-profiles').addEventListener('click', () => void loadProfiles());
  $('profile-list').addEventListener('click', event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-profile]');
    if (button) void readProfile(button.dataset.profile!);
  });
  $('profile-form').addEventListener('input', editorChanged);
  $('profile-form').addEventListener('change', editorChanged);
  $('store').addEventListener('input', () => {
    profiles.generation++;
    editorChanged();
    $('profile-list').replaceChildren();
  });
  $('profile-form').addEventListener('submit', event => {
    event.preventDefault();
    void saveProfile();
  });
  $('save-config').addEventListener('click', () => saveConfiguration(navigate));
  $('use-profile').addEventListener('click', () => useProfile(navigate));
  $('delete-profile').addEventListener('click', () => {
    if (profiles.mutationBusy || profiles.loading) return;
    profiles.deleteTarget = { store: control('store').value, name: control('profile-name').value, revision: profiles.revision };
    $('delete-confirm').hidden = false;
  });
  $('keep-profile').addEventListener('click', () => {
    profiles.deleteTarget = null;
    $('delete-confirm').hidden = true;
  });
  $('confirm-delete').addEventListener('click', () => void deleteProfile());
}
