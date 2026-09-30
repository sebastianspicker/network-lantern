import { defineConfig } from '@playwright/test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
export default defineConfig({testDir:'tests',testMatch:'browser.spec.ts',outputDir:join(tmpdir(),'lantern-desktop-browser-results'),reporter:'list',use:{baseURL:'http://127.0.0.1:1420'},webServer:{command:'npm run dev',url:'http://127.0.0.1:1420',reuseExistingServer:true}});
