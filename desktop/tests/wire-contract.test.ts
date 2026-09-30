import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { active, type Progress } from '../src/model';

// The Rust runtime test `wire_enums_match_the_shared_fixture` checks the same fixture against serde.
const wire = JSON.parse(
  readFileSync(new URL('../../tests/fixtures/contracts/wire-enums.json', import.meta.url), 'utf8'),
) as { run_state: string[]; error_category: string[] };

const browserSpec = readFileSync(new URL('./browser.spec.ts', import.meta.url), 'utf8');

describe('Rust wire contract', () => {
  it('treats exactly the running and cancelling run states as active', () => {
    const activeStates = wire.run_state.filter(state => active({ state } as Progress));
    expect(activeStates).toEqual(['running', 'cancelling']);
  });

  it('uses only real error categories in browser fixtures', () => {
    const used = new Set([...browserSpec.matchAll(/category:\s*'([a-z_]+)'/g)].map(match => match[1]));
    expect(used.size).toBeGreaterThan(0);
    for (const category of used) expect(wire.error_category).toContain(category);
  });
});
