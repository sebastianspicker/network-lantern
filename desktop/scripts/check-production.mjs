import { readdirSync, readFileSync } from 'node:fs';
const directory=new URL('../dist/assets/',import.meta.url);
for(const file of readdirSync(directory).filter(name=>name.endsWith('.js'))){
  const source=readFileSync(new URL(file,directory),'utf8');
  if(/wdioTauri|__wdio_mocks__|plugin:wdio|WDIO Plugin/.test(source))throw new Error(`Test instrumentation found in production asset ${file}`);
}
console.log('Production assets exclude WDIO instrumentation.');
