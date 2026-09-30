import type { Doctor, Flow, Progress, Request } from './model';

export interface AppState {
  flow: Flow;
  // Last measurement flow, used as the default destination when saving a configuration.
  previousFlow: Flow;
  doctor: Doctor | null;
  run: Progress | null;
  // Fingerprint and request of the reviewed plan; empty when the preview is stale.
  previewKey: string;
  previewRequest: Request | null;
  loadedWorkflowEnvelope: boolean;
  // Incremented whenever the preview is invalidated so late plan responses are ignored.
  generation: number;
  starting: boolean;
  planning: boolean;
  helperBusy: boolean;
  polling: boolean;
  // Incremented on navigation so library responses from a previous visit are ignored.
  libraryRevision: number;
}

export const state: AppState = {
  flow: 'triage',
  previousFlow: 'triage',
  doctor: null,
  run: null,
  previewKey: '',
  previewRequest: null,
  loadedWorkflowEnvelope: false,
  generation: 0,
  starting: false,
  planning: false,
  helperBusy: false,
  polling: false,
  libraryRevision: 0,
};
