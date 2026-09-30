# Desktop frontend

The desktop uses TypeScript and Vite, wrapped by the native Tauri host in
`src-tauri`. Planning, measurement, profile persistence, report operations, and
capability checks all go through Rust commands. A plain browser can still render
the interface, but it reports that the native runtime is unavailable and does not
simulate a successful result.

## Source layout

- `src/main.ts` mounts the page and wires the modules together.
- `src/markup.ts` holds the static page structure. Element ids are a contract with
  the browser and native tests; `tests/fixtures/dom-contract.json` records them.
- `src/state.ts` is the single view-state object, including the generation
  counters that discard stale responses.
- `src/model.ts` (requests, fingerprints, run summaries) and `src/render.ts`
  (escaped markup fragments) are pure and unit-tested.
- `src/plan.ts`, `run.ts`, `profiles.ts`, `reports.ts`, `runtime.ts`,
  `navigation.ts`, `measurement.ts` and `library.ts` each own one view concern.
- `src/bridge.ts` is the only module that calls Tauri. Browser tests replace it by
  path, so keep its location and its `native` and `command` exports.

## Commands

From this directory:

```sh
npm ci
npm run check
npm test
npm run build
npm run test:browser
npm run build:e2e
npm run test:native
```

`npm run dev` serves the interface at `http://127.0.0.1:1420`. Browser tests use
that exact address and start the server if it is not already running. Screenshots
and test artifacts go to the operating system temporary directory.

The native test build enables the Cargo `e2e` and embedded-asset protocol
features, merges `src-tauri/tauri.e2e.conf.json`, and builds Vite in `e2e` mode.
The production frontend excludes the WebdriverIO plugin. Native tests drive real
Rust commands through the embedded WebDriver provider, using temporary local
profile and report files; one cancellation test opens a bounded local TCP
listener. They do not run external probes or privileged tuning.

The seven flows share one review-then-start lifecycle. Changing a field, output
path, workflow, or loaded profile invalidates the preview. A single active run
stays visible across flows, progress polling runs at four non-overlapping polls
per second, and cancellation targets the active run identifier. Recent activity
is capped at 64 entries. Reports load in pages of at most 20 rows, with missing
metrics shown as unavailable.

Profile and result path fields are passed to Rust, so use explicit absolute paths
when the launch working directory is uncertain. After loading a profile, review
the resolved plan: its JSON settings form the final override layer. Platform
capabilities and helper limitations appear as the runtime reports them.
