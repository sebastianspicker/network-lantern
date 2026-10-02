# Network Lantern design brief

Status: direction chosen and implemented on branch `design/logbook-and-lamp`.
Scope: the Tauri desktop interface (`desktop/src`) and the static command planner
(`site/`). The CLI, PowerShell/Bash reference, and Rust engines have no visual
surface and are out of scope.

## 1. Product

Network Lantern collects path and throughput evidence from a source checkout and,
optionally, inspects or changes a small set of Windows network settings. It has
four independent capabilities (basic path diagnostics, continuous path trace,
iperf3-compatible throughput, and Windows tuning). They are composed into five
workflows: Triage, Path, Throughput, Baseline, and Windows tuning.

Two visual surfaces:

| Surface | What it does | Where it runs |
| --- | --- | --- |
| Desktop (`desktop/`) | Configure a workflow, have Rust resolve a plan, start one run, watch progress, cancel it, keep profiles, browse and compare reports, and manage the privileged helper | Tauri webview, 1200×820 default window, 720×540 minimum |
| Planner (`site/`) | Build a `network-lantern workflow … --dry-run` command in the browser and copy it | GitHub Pages; no probes, network, or storage, which tests enforce |

Its ethic is unusually explicit, and the design should follow it:

- **Planning is separate from execution.** `--dry-run` "never resolves DNS,
  connects sockets, authorizes a helper or writes results" (CLI help).
- **Review before start.** In the desktop, any change invalidates the preview, and
  Start stays disabled until a fresh Rust plan exists.
- **Evidence over claims.** "Missing metrics remain unavailable." Reports keep
  provenance. Nothing is simulated when the runtime is absent.

### Moment of value

There are two. The second is rarer and depends on the first.

1. **The reviewed plan.** The operator sees exactly what will happen before any
   packet leaves the machine: which hosts get traffic, how many tests run, how
   long they take, and whether Windows will be changed. The Rust plan already
   contains this (`total_tests: 1145`, `estimated_test_seconds: 12595`,
   `within_budget`, `mutating: true`, ordered tuning steps), but the current UI
   reduces it to "workflow: 1 planned items" plus raw JSON. **This is the single
   largest gap between product substance and interface.**
2. **The recorded evidence.** This is a run's report with status, measurement
   count, provenance, and comparison with a baseline.

### Key journeys

1. *Triage a complaint* ("the VPN is slow"): Triage → host + iperf3 server →
   review plan → notice 1,145 tests ≈ 3.5 h → set a budget → review again →
   start → watch → open report.
2. *Path only*: Path → host, family, engine (basic/trace) → review → start.
3. *Windows tuning*: Verify (read-only) → Backup → Apply (mutating, needs helper)
   → Restore. Each step must be recognisable as harmless or consequential.
4. *Repeatable checks*: save configuration as profile → later load it → review
   again (profiles never bypass review).
5. *Compare*: Reports → open a run → page rows → compare with a baseline → export.
6. *Planner*: pick a workflow → fill host/server → copy dry-run command → paste
   into a terminal.

## 2. Audience

**Primary:** a network or systems engineer, often the one person in IT at a school,
lab, studio, or small office, who has been asked to prove where a network problem
is. They are Windows-heavy, comfortable in PowerShell and a Unix shell, and
already use `ping`, `tracert`, `mtr`, `iperf3`, and Wireshark.

- **Goals:** produce defensible evidence (to an ISP, a vendor, a manager), find
  the hop or setting at fault, and not make anything worse.
- **Anxieties:** sending load across a shared or production link, testing a
  target they aren't authorised for, changing Windows QoS without a way back,
  a run that silently takes hours, and losing the numbers afterwards.
- **What they distrust:** glossy dashboards that hide raw output; speedometer
  gauges and "health scores"; tools that phone home; any UI that rounds away or
  invents data; marketing language.
- **What signals quality to them:** exact figures with units; monospace where
  values are compared; the actual command or JSON one click away; honest states
  ("Runtime unavailable", not a spinner forever); legible addresses (`0`/`O`,
  `1`/`l`/`I` distinguishable); keyboard operability; restraint.

**Secondary:** maintainers extending one capability at a time, reading the UI as
a mirror of the Rust contract, and evaluators arriving from the README or GitHub
Pages who judge the project by the planner.

## 3. Brand traits

| Trait | Not |
| --- | --- |
| **Exact**: every value has a unit, a source, and a place | pedantic or cluttered with every field |
| **Calm under load**: designed for 2 a.m. outage work | sleepy or low-contrast |
| **Candid**: says what will happen and what's unknown | alarmist, or hedging everything |
| **Instrument-grade**: feels like a tool you own | skeuomorphic or nostalgic |
| **Quietly warm**: a lantern, not a floodlight | cute, mascot-driven, or "friendly SaaS" |

## 4. Market observations

These come from category knowledge. No live web research was run in this session
(see assumption A7). Close alternatives: PingPlotter, WinMTR, SmokePing,
ThousandEyes/Kentik-style SaaS monitors, Ookla and Cloudflare speed tests,
iperf3 front-ends (jperf, iperf3 GUIs), and Wireshark.

Category conventions:

- **NOC dark mode**: near-black with neon green/cyan traces. Signals "hacker
  tool", is tiring for long forms, and is now a cliché. *Break it.*
- **Enterprise blue dashboards** with KPI tiles and donut charts (SolarWinds,
  monitoring SaaS). *Break it.* Network Lantern is a run-based instrument, not a
  monitor.
- **Speed-test gauges**. *Refuse.* They imply one number answers the question.
- **Dense tabular output** (WinMTR, mtr, Wireshark packet list). *Honor it.*
  Operators read columns. Tabular figures and monospace values are expected.
- **Status colour (green/amber/red)**. *Honor it, sparingly.* Colour must mean
  consequence, never decoration.

## 5. Current state

Stack: vanilla TypeScript and Vite markup strings (`markup.ts`, `render.ts`) with
one global `style.css`. The planner is static HTML, CSS, and two IIFE scripts.
There are no tokens beyond four CSS variables, and the two surfaces duplicate
values (`#146255`, `#142b39`, `#d4dde2`).

**Keep:**

- The **lantern mark**: a lantern inside a circle, amber stroke (`#d99512` /
  `#e6a019`). It is the only recognisable brand asset and the amber is the
  strongest colour equity. Evolve it, don't discard it.
- The two-pane **configure | preview** structure, which mirrors the
  plan/execute boundary.
- All DOM ids, ARIA roles, the planner's tab semantics, and the copy that tests
  pin as behaviour (state strings, error strings).

**Weaknesses:**

- **Generic SaaS fingerprint**: a system font stack, deep-teal filled primary
  buttons, 8px-radius white card on grey, a centered marketing H1 ("Investigate
  your network.") above an app, and stock line icons on the planner tabs.
- **Hierarchy**: on the desktop, the most important thing (what will happen) is
  a numbered circle reading "workflow / 1 planned items". The marketing H1 and
  intro take the top of the window.
- **Colour has no meaning**: teal is used for navigation, primary actions,
  focus, steps and links alike. "Start" (sends traffic) looks the same as
  "Review plan" (harmless).
- **Run state is weak**: an active run is a plain white strip with a native
  `<progress>`.
- **Unglamorous states**: library tables use `secondary` button styling for
  everything, there's no empty-list treatment, raw `<pre>` blocks for report
  rows, and focus rings are uniform teal.
- **Light mode only.** `color-scheme: light` is hard-coded.

## 6. Constraints (load-bearing)

- **Contracts**: DOM ids (`desktop/tests/fixtures/dom-contract.json`), Tauri
  command names and payloads (`requests` contract), `bridge.ts` path and exports,
  planner ids and `<select>` single-line format (the planner tests parse the HTML
  with regexes), and `textContent`-based command rendering in the planner.
- **Pinned strings**: run state titles (`cancelled`, …), `Runtime unavailable`,
  `Settings changed. Review a fresh plan before starting.`, `Plan ready. …`,
  profile and report status messages, and validation messages. Behaviour copy
  stays; static copy can be rewritten.
- **Security**: the desktop CSP is `default-src 'self'; style-src 'self'
  'unsafe-inline'`, so fonts and images must be bundled. No CDN. The planner
  must not make network requests or persist anything; tests ban
  `fetch`/storage APIs. **Fonts are self-hosted on both surfaces.**
- **Platform**: Tauri webviews are WebKit on macOS, WebView2 on Windows, and
  WebKitGTK on Linux. Use only well-supported CSS (no `@scope`, and no anchor
  positioning without a fallback).
- **Accessibility**: WCAG 2.2 AA, full keyboard use, visible focus, and
  `prefers-reduced-motion`.
- **Performance**: no runtime dependencies added; fonts are two variable WOFF2
  files (latin), ~52 KB in total, with `font-display: swap` and metric-matched
  fallbacks to limit layout shift.

## 7. Assumptions log

| # | Assumption | Evidence | Confidence |
| --- | --- | --- | --- |
| A1 | The primary user is a hands-on network/sysadmin generalist, Windows-first | Windows tuning capability, PowerShell reference, `pathping`/`tracert`, Windows Forms client, README "operators who diagnose networks they are authorized to test" | High |
| A2 | The desktop is used mostly at ≥1000px wide; the 390px layout matters only for browser checks and small windows | Tauri `minWidth: 720`, default 1200×820; browser test asserts no overflow at 390 | High |
| A3 | The planner is the project's public face and is often seen on phones (links shared in chat) | GitHub Pages deploy, README hero screenshot, existing mobile screenshot | Medium |
| A4 | Users want dark mode, especially during incident work at night | Domain norm in terminals/IDEs; no explicit request in repo | Medium |
| A5 | Showing plan-derived facts (targets, test count, duration, budget, mutation) is presentation, not new behaviour, and Rust stays the authority | `plan` command already returns these fields; README: "that CLI's preview remains the source of truth for test counts, duration estimates, budget checks" | High |
| A6 | Plan field names (`total_tests`, `estimated_test_seconds`, `within_budget`, `mutating`, `requiresHelper`, `families`, `ipv4_hosts`, `config.target`) are stable enough to display, with a graceful fallback when absent | Observed in `network-lantern workflow … --dry-run` output; `plannedItems()` already relies on `total_tests`/`total_items` | Medium |
| A7 | Category conventions are as described in §4 without live verification | Category knowledge; no web research performed in this session | Medium |
| A8 | Amber as a fill with dark ink text meets AA, and amber as text needs a darker tone on light backgrounds | Contrast is computed for every token pair (see §9) | High |
| A9 | Atkinson Hyperlegible Next/Mono render consistently in WebKit, WebView2, and WebKitGTK | Standard variable WOFF2 with `font-weight` ranges; supported in all three engines | High |

## 8. Design direction

The domain offers raw material in its artefacts (traceroute hop tables, iperf3
interval logs, the operator's logbook of what was tested when and on whose
authority), in its rituals (preview, then run, then keep the record), and in its
name: a lantern, a small portable light carried into a dark place, lit only when
needed.

### Direction A: **Logbook & Lamp** (chosen)

**Concept.** The interface is an operator's logbook, and the lantern is lit only
when the network will feel it. Configuring and reviewing happen in calm ink on
paper. The amber lamp appears in exactly two places: the Start action and the
active-run strip (plus the lantern's flame in the mark). Red appears only where
something can be destroyed or changed (Windows changes, delete, cancel). **Colour
is consequence.** Plans read like logbook entries: numbered, ruled, and set in
tabular figures, stating *what will happen* before it happens.

*Why it fits:* it turns the product's safety ethic (review before execution,
evidence over claims) into the visual grammar itself, and speaks to an
audience whose chief anxiety is unintended consequence.

**Typography.** Atkinson Hyperlegible Next (UI, variable 200–800) with Atkinson
Hyperlegible Mono (every value the network will see: hosts, ports, counts,
durations, commands, JSON). The pairing has a domain reason: Atkinson was drawn
to separate commonly confused characters, which is exactly the
`0/O`, `1/l/I`, `rn/m` problem in hostnames and IPv6 addresses. Headings use
Next at weight 700 with tight tracking, and labels are small-caps-style uppercase
at 11–12px with wide tracking (the logbook's column headings). The scale is
11 · 12 · 13 · 15 · 17 · 21 · 28 · 40 (≈1.25 ratio from 13, tuned by hand). All
figures are `tabular-nums`.

**Colour.** Paper and ink do the work, with two signal colours and one quiet
confirmation colour.

| Role | Light | Dark ("night shift") | Use |
| --- | --- | --- | --- |
| `--paper` | `#ffffff` | `#0d0f12` | canvas, input fields |
| `--sheet` | `#f5f6f8` | `#15181c` | selected and raised surfaces |
| `--ink` | `#0d1117` | `#e8eaed` | text, primary rules, primary buttons |
| `--ink-2` | `#4a515c` | `#a7adb5` | secondary text |
| `--ink-3` | `#5f6773` | `#8a919a` | hints, column labels |
| `--rule` / `--rule-strong` | `#e2e5e9` / `#b9bfc7` | `#262a30` / `#3d434b` | hairlines |
| `--lamp` | `#efa531` | `#f2ae45` | lit state fills: Start, active run |
| `--lamp-text` | `#7d4b00` | `#f2ae45` | amber text on paper |
| `--alert` | `#b0281f` | `#ff8f80` | destructive and mutating |
| `--ok` | `#1c6656` | `#6cc4a9` | verified/complete (the old brand teal, demoted to one role) |

No gradients, no tints on everything, and no blue.

**Layout.** A ledger grid. The desktop has a 232px index rail (workflows
numbered 01–05, grouped *Measure* / *Change* / *Records*), then a
settings column and a plan column on a 4px baseline with 1px ruled dividers, and
no cards. Density is medium-high, since operators scan. Labels sit above values,
and long values wrap rather than truncate. The planner keeps the same rail,
form, and manifest, with the command "slip" as the culmination.

**Motion.** Almost none. The lamp indicator on an active run breathes (opacity
only, 2.4s), the progress bar moves by `transform: scaleX`, and focus and hover
take 120ms. All of it is disabled under `prefers-reduced-motion`.

**Signature details.**

1. **The plan manifest.** Each step is a numbered entry with a facts grid:
   *Sends traffic to* (hosts in mono), *Planned* (`1,145 tests`), *Nominal
   time* (`≈ 3 h 30 min`), *Budget* (`No limit set` / `Over budget`), *Changes
   Windows* (`Yes, needs helper` in alert ink) and ordered tuning steps.
   Unknown values say "See resolved plan".
2. **Lit only when it counts.** The amber Start button and run strip are the
   only lamp-coloured surfaces. The idle interface is monochrome, so the
   operator learns that amber means "the network will feel this".
3. **Lifecycle rule.** `Configure → Review → Run` sits above the plan and fills
   as the preview becomes fresh, making the review-before-start rule visible.

**Category stance.** It is light-first, honours tables and monospace, refuses
NOC neon, refuses gauges and KPI tiles, and uses colour semantically.

**Refuses.** Gradients, glass, illustrations, icon sets in tinted circles,
shadows for depth, rounded cards, marketing hero copy inside the app, and emoji.

### Direction B: Light List (night chart)

*Concept:* the Admiralty List of Lights and nautical charts. Each workflow is a
light with a "characteristic" (`Fl(2) 10s`), and the plan is a chart notation.
It is dark-first: a deep navy-ink canvas, chart-cream type, and amber light
marks. *Type:* a condensed grotesk (Barlow Condensed) for headings and
IBM Plex Mono for data. *Layout:* a full-bleed chart canvas with the path drawn
as soundings between hops. *Signature:* light characteristics as compact codes;
hop depth as soundings. *Stands apart:* it is nautical rather than NOC.
*Refuses:* neon. **Rejected:** dark-first edges back toward the NOC cliché, the
nautical metaphor is clever but asks operators to learn a second notation, and a
chart canvas needs per-hop data the desktop doesn't render yet.

### Direction C: Bench Instrument

*Concept:* the front panel of lab test gear (signal generator, network
analyzer), with grey enamel panels, engraved small-caps labels, segmented
readouts, and physical toggles for options. *Type:* DIN-style (Barlow) with a
segmented display face for figures. *Layout:* fixed "panels" bolted to a
chassis grid. *Signature:* an engraved panel legend and readout windows for
counts. **Rejected:** it is skeuomorphic and nostalgic (it tips "instrument-grade"
into "costume"), segmented figures are harder to read and copy, toggle
metaphors weaken native form semantics, and it ages fast.

### Choice

**A, Logbook & Lamp.** It is the only direction whose organizing idea comes
directly from the product's own rule (plan, then lit execution, then record),
and it puts the effort where the brief found the largest gap: the reviewed plan.
It is legible in both themes, works with native controls, and the typeface
choice has a functional justification the audience will feel without being
told.

*Traded away:* B's drama and memorability on first sight, and C's tactile
delight. A is quieter; its distinctiveness comes from typographic precision
and disciplined colour, which reward use more than a screenshot does.

## 9. Verification targets

- Contrast is computed for every text/background token pair in both themes,
  with a minimum of 4.5:1 for text and 3:1 for UI boundaries and focus rings.
- Screens are captured at 390, 834, and 1440 px in light and dark, for every
  flow and for the empty, error, running, and terminal states.
- The full gate passes (`./scripts/ci-local.sh`), including the regenerated DOM
  contract.

## 10. Build notes and revisions

- **Palette (revision after user review).** The first palette used warm
  off-white "paper" (`#f4f3ee`) and warm graphite. The user rejected it as the
  generic beige look of AI-generated design. It was replaced with white and cool
  neutral greys with blue-black ink, in both themes. Layout, type, and the
  amber/red consequence rule are unchanged. Because the tokens are shared, the
  desktop changed too.

- **Planner seal (revision).** The first plan gave the planner's "Dry run" seal
  the lamp colour. Building it showed that this contradicts the concept: a dry
  run is the one thing guaranteed not to touch the network. The seal uses the
  verified green (`--slip-ok`) instead. Amber stays reserved for consequence.
- **Over budget is an error, not a plan.** Rust rejects an over-budget throughput
  plan with a validation error, so the manifest's `within_budget: false` branch
  is a defensive fallback. The common case renders in the error style.
- **Inputs need their own border token.** Hairline rules (`--rule`) are about
  2:1 against the surface, below the 3:1 a control boundary needs. Inputs and
  outlined buttons use `--field` (3.49:1 light, 3.31:1 dark). The lit Start
  button has a darker `--lamp-edge` border, because amber on paper alone is
  1.87:1.
- **Review evidence.** Captures were rendered in WebKit (the macOS Tauri engine)
  at 390, 834, 1200, and 1440 px, light and dark, for every workflow and for the
  plan ready, plan error, runtime unavailable, running, partial failure,
  profiles, empty, and reports states. The planner was captured at the same
  widths. No horizontal overflow at 390 px.

### Robustness against the low-confidence assumptions

- *A3 (planner seen on phones):* if wrong, nothing is lost. The phone layout is
  a separate sequence and costs desktop nothing.
- *A4 (dark mode wanted):* it follows the system setting with no toggle, so
  light-mode users never see it.
- *A6 (plan field names stable):* each fact is optional, and an unknown shape
  degrades to "Planned: See resolved plan" plus the JSON. Unit tests pin the
  shapes in use today.
- *A7 (category conventions unverified):* the direction does not depend on any
  competitor's look. Its rules come from the product's own review-before-run
  discipline.

### Unresolved

- WebView2 (Windows) and WebKitGTK (Linux) renders are unverified on real hosts.
  The CSS avoids features those engines lack, except `:has()` for the sticky-plan
  offset, which degrades to a minor overlap.
- The desktop placeholder `iperf.example.net` differs from the planner default
  `iperf3.example.net`. This was left unchanged to keep the DOM contract churn
  to the redesign itself.
- Report rows are still shown as JSON. A per-capability measurement table (hops
  for path, intervals for throughput) is the next step toward the "recorded
  evidence" moment of value.
