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

test('landing shows a static poster before the animated iframe is ready',()=>{
  const landing=read('../site/index.html');
  const demo=read('../site/demo.js');
  assert.match(landing,/<link rel="preload" as="image" href="\.\/kind-of-blue\.jpg" fetchpriority="high">/);
  assert.match(landing,/<div class="preview-poster" aria-hidden="true">/);
  assert.match(landing,/<img class="poster-cover" src="\.\/kind-of-blue\.jpg"/);
  assert.match(landing,/<div class="poster-meta">So What<span>Miles Davis<\/span><\/div>/);
  assert.match(landing,/\.preview iframe\{[^}]*opacity:0/);
  assert.match(landing,/\.preview\[data-ready='true'\] iframe\{opacity:1\}/);
  assert.match(landing,/event\.origin!==location\.origin\|\|event\.source!==previewFrame\.contentWindow/);
  assert.match(demo,/image\.decode\(\)\.catch\(\(\)=>\{\}\)\.then\(show\)/);
  assert.match(demo,/postMessage\('soloist-demo-ready'/);
});
