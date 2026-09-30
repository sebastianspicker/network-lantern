import { errorMessage, escape as e, plannedItems, type Json, type RunsPage } from './model';

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

export function planSummary(result: Json): string {
  const steps = (result.steps as Json[] | undefined) || [result];
  const items = steps.map(step => `<li><strong>${e(step.capability)}</strong><p>${e(plannedItems(step))} planned items</p></li>`);
  const warnings = ((result.warnings as string[]) || []).map(warning => `<p class="notice">${e(warning)}</p>`);
  return `<ol class="steps">${items.join('')}</ol>${warnings.join('')}`;
}

export function profileList(names: string[]): string {
  return names.map(name => `<li><button class="secondary" data-profile="${e(name)}">${e(name)}</button></li>`).join('');
}

export function runList(runs: RunsPage['runs']): string {
  return runs.map(item => {
    const cells = [
      item.summary?.timestamp || item.path,
      item.summary?.status || errorMessage(item.error || 'Unavailable'),
      item.summary?.total ?? 'Unavailable',
    ].map(cell => `<td>${e(cell)}</td>`);
    return `<tr>${cells.join('')}<td><button class="secondary" data-report="${e(item.path)}">Open</button></td></tr>`;
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
