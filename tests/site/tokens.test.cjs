const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { test } = require('node:test');
const root = resolve(__dirname, '../..');
const read = (path) => readFileSync(resolve(root, path));

test('planner and desktop share identical design tokens and fonts', () => {
  for (const file of ['tokens.css', 'fonts/atkinson-hyperlegible-next.woff2', 'fonts/atkinson-hyperlegible-mono.woff2', 'fonts/OFL.txt']) {
    assert.ok(read(`site/${file}`).equals(read(`desktop/src/${file}`)), `site/${file} differs from desktop/src/${file}`);
  }
});

test('tokens load fonts from bundled files only', () => {
  const tokens = read('site/tokens.css').toString();
  for (const [, url] of tokens.matchAll(/url\("([^"]+)"\)/g)) {
    assert.match(url, /^(?:\.\/fonts\/[\w-]+\.woff2|data:image\/svg\+xml,)/, url);
  }
});
