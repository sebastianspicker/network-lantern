import { command } from './bridge';
import { $ } from './dom';
import { updateAvailability } from './measurement';
import { active, errorMessage, helperDetail, type Doctor, type Json } from './model';
import { state } from './state';

export async function checkRuntime() {
  $('environment').textContent = 'Checking runtime…';
  try {
    const doctor = await command<Doctor>('doctor');
    state.doctor = doctor;
    $('environment').textContent = `${doctor.os} · ${doctor.architecture} · ${doctor.version}`;
    $('runtime-notice').textContent = 'Local runtime ready. Review capability permissions before starting.';
    $('helper-detail').textContent = helperDetail(doctor.helper);
  } catch (error) {
    state.doctor = null;
    $('environment').textContent = 'Runtime unavailable';
    $('helper-detail').textContent = 'Helper status unavailable while the native runtime is disconnected.';
    $('runtime-notice').textContent = errorMessage(error);
  }
  updateAvailability();
}

// Registration and removal are exclusive with each other and with starting a run.
async function changeHelper(operation: 'register' | 'remove') {
  if (state.helperBusy || active(state.run) || state.starting) return;
  state.helperBusy = true;
  updateAvailability();
  $('helper-detail').textContent = operation === 'register' ? 'Waiting for helper registration authorization…' : 'Removing helper…';
  try {
    const status = await command<Json>(`helper_${operation}`);
    $('helper-detail').textContent = String(status.detail || status.registration);
    await checkRuntime();
  } catch (error) {
    $('helper-detail').textContent = errorMessage(error);
  } finally {
    state.helperBusy = false;
    updateAvailability();
  }
}

export function setupRuntime() {
  $('refresh-runtime').addEventListener('click', () => void checkRuntime());
  $('helper-register').addEventListener('click', () => void changeHelper('register'));
  $('helper-remove').addEventListener('click', () => void changeHelper('remove'));
}
