import { expect, test } from 'vitest';
import { nativeEnvironment } from '../scripts/native-environment.mjs';
test('native diagnostics receive platform essentials without unrelated credentials', () => {
  const filtered = nativeEnvironment({
    PATH: '/fixture/bin', HOME: '/fixture/home', SystemRoot: 'C:\\Windows',
    DISPLAY: ':99', XDG_RUNTIME_DIR: '/run/user/fixture',
    EXAMPLE_API_KEY: 'fixture-only', GITHUB_TOKEN: 'fixture-only',
    AWS_SECRET_ACCESS_KEY: 'fixture-only', NODE_OPTIONS: '--require=untrusted.js',
  });
  expect(filtered).toEqual({PATH:'/fixture/bin',HOME:'/fixture/home',SystemRoot:'C:\\Windows',DISPLAY:':99',XDG_RUNTIME_DIR:'/run/user/fixture'});
});
