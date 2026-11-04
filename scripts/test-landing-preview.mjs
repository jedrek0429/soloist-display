import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const read=path=>readFileSync(new URL(path,import.meta.url),'utf8');

test('landing preview starts in playing state before JavaScript loads',()=>{
  const workflow=read('../.github/workflows/pages.yml');
  assert.match(workflow,/s@data-display-state="connecting"@data-display-state="playing"@/);
  assert.match(workflow,/s@src="\.\/src\/main\.js"@src="\.\/src\/demo\.js"@/);
  const generated=read('../index.html')
    .replace('data-display-state="connecting"','data-display-state="playing"')
    .replace('src="./src/main.js"','src="./src/demo.js"');
  assert.match(generated,/<html[^>]*data-display-state="playing"/);
  assert.match(generated,/<script type="module" src="\.\/src\/demo\.js"/);
  assert.match(generated,/<section class="standby"/);
});

test('demo renderer does not leave standby covering artwork',()=>{
  const demo=read('../site/demo.js');
  assert.match(demo,/root\.dataset\.displayState='playing'/);
  assert.match(demo,/document\.getElementById\('track'\)\.textContent='So What'/);
  assert.match(demo,/document\.getElementById\('artist'\)\.textContent='Miles Davis'/);
  assert.match(read('../site/index.html'),/<iframe src="\.\/preview\.html"/);
});
