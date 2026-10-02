import { errorMessage, escape as e, planEntry, type Json, type PlanEntry, type RunsPage } from './model';

export type Attributes = Record<string, string | true>;

export function attributes(attrs: Attributes): string {
  return Object.entries(attrs).map(([name, value]) => (value === true ? name : `${name}="${e(value)}"`)).join(' ');
}

export function field(id: string, label: string, value = '', type = 'text', attrs: Attributes = {}): string {
  const input = `<input id="${e(id)}" type="${e(type)}" value="${e(value)}" ${attributes(attrs)}>`;
  return `<label class="field" for="${e(id)}"><span>${e(label)}</span>${input}</label>`;
}

export type Option = string | { value: string; label: string };

export function options(values: readonly Option[]): string {
  return values
    .map(option => (typeof option === 'string'
      ? `<option>${e(option)}</option>`
      : `<option value="${e(option.value)}">${e(option.label)}</option>`))
    .join('');
}

export function select(id: string, label: string, values: readonly Option[]): string {
  return `<label class="field" for="${e(id)}"><span>${e(label)}</span><select id="${e(id)}">${options(values)}</select></label>`;
}

function manifestEntry(entry: PlanEntry): string {
  const facts = entry.facts.map(fact => `<div class="fact${fact.wide ? ' wide' : ''}"${fact.tone ? ` data-tone="${fact.tone}"` : ''}>`
    + `<dt>${e(fact.label)}</dt><dd>${e(fact.value)}</dd></div>`);
  const operations = entry.operations.length
    ? `<ol class="operations" aria-label="Ordered operations">${entry.operations.map(item => `<li>${e(item)}</li>`).join('')}</ol>`
    : '';
  const warnings = entry.warnings.map(warning => `<p class="notice" data-tone="alert">${e(warning)}</p>`);
  return `<li class="entry"><h3>${e(entry.title)}</h3><dl class="facts">${facts.join('')}</dl>${operations}${warnings.join('')}</li>`;
}

// A workflow plan lists its capability steps; a single-capability plan is its own step.
export function planSummary(result: Json): string {
  const steps = (result.steps as Json[] | undefined) || [result];
  const warnings = ((result.warnings as string[]) || []).map(warning => `<p class="notice" data-tone="alert">${e(warning)}</p>`);
  return `<ol class="manifest">${steps.map(step => manifestEntry(planEntry(step))).join('')}</ol>${warnings.join('')}`;
}

export function profileList(names: string[]): string {
  return names.map(name => `<li><button class="index-item" data-profile="${e(name)}">${e(name)}</button></li>`).join('');
}

export function runList(runs: RunsPage['runs']): string {
  return runs.map(item => {
    const status = item.summary?.status ? String(item.summary.status) : '';
    const label = status || errorMessage(item.error || 'Unavailable');
    const cells = [
      `<td class="data">${e(item.summary?.timestamp || item.path)}</td>`,
      `<td><span class="status" data-status="${e(status || 'unavailable')}">${e(label)}</span></td>`,
      `<td class="figure">${e(item.summary?.total ?? 'Unavailable')}</td>`,
    ];
    return `<tr>${cells.join('')}<td class="row-action"><button class="secondary compact" data-report="${e(item.path)}">Open</button></td></tr>`;
  }).join('');
}

export function reportMetadata(path: string, metadata: Json): string {
  const entries = [
    ['File', e(path)],
    ['Status', e(metadata.status || 'Unavailable')],
    ['Source', e(metadata.source_schema)],
    ['Provenance', `<code>${e(JSON.stringify(metadata.provenance))}</code>`],
  ];
  return `<dl>${entries.map(([term, detail]) => `<dt>${term}</dt><dd>${detail}</dd>`).join('')}</dl>`;
}
