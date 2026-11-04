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

test('old metadata placeholders cannot flash during startup or reconnect',()=>{
  const html=read('../index.html');
  const app=read('../src/main.js');
  const style=read('../styles.css');
  const demo=read('../site/demo.js');
  assert.match(html,/<html[^>]*data-metadata-ready="false"/);
  assert.match(html,/<h1 id="track"><\/h1><p id="artist"><\/p>/);
  assert.doesNotMatch(html,/Not playing|Waiting for Soloist/);
  assert.doesNotMatch(app,/Not playing|Waiting for Soloist/);
  assert.match(app,/root\.dataset\.metadataReady='false'/);
  assert.match(app,/root\.dataset\.metadataReady=name\.trim\(\)\?'true':'false'/);
  assert.match(style,/:root:not\(\[data-metadata-ready='true'\]\) \.info\{visibility:hidden!important\}/);
  assert.match(demo,/root\.dataset\.metadataReady='true'/);
});

test('landing shows only the real renderer, with a black loading frame',()=>{
  const landing=read('../site/index.html');
  const demo=read('../site/demo.js');
  assert.match(landing,/<link rel="preload" as="image" href="\.\/kind-of-blue\.jpg" fetchpriority="high">/);
  assert.match(landing,/<link rel="preload" as="style" href="\.\/styles\.css">/);
  assert.match(landing,/\.preview\{[^}]*background:#151515/);
  assert.match(landing,/<iframe src="\.\/preview\.html"[^>]*loading="eager"/);
  assert.doesNotMatch(landing,/preview-poster|poster-art|poster-vinyl|poster-cover|poster-meta|data-ready|postMessage/);
  assert.doesNotMatch(demo,/soloist-demo-ready|image\.decode\(\)/);
  assert.match(demo,/root\.dataset\.displayState='playing'/);
});
