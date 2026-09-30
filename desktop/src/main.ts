import './style.css';
import { $ } from './dom';
import { appMarkup } from './markup';
import { navigate, setupNavigation } from './navigation';
import { setupPlan } from './plan';
import { setupProfiles } from './profiles';
import { setupReports } from './reports';
import { setupRun, startPolling } from './run';
import { checkRuntime, setupRuntime } from './runtime';

if (import.meta.env.MODE === 'e2e') await import('@wdio/tauri-plugin');

$('app').innerHTML = appMarkup;
setupNavigation();
setupPlan();
setupRun();
setupRuntime();
setupProfiles(navigate);
setupReports();

navigate('triage');
$('plan-state').textContent = 'Configure your workflow and review its plan.';
void checkRuntime();
startPolling();
