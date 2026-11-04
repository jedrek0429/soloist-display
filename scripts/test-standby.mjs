import test from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import vm from 'node:vm';
import {isEmptyPlayback,pauseDelayMs} from '../src/soloist.js';

test('disc and standby use separate vinyl labels and setting keys',()=>{
  const source=fs.readFileSync(new URL('../src/settings.js',import.meta.url),'utf8');
  const element={addEventListener(){}};
  const context=vm.createContext({
    document:{querySelector:()=>element},
    fetch:async()=>{throw Error('offline');}
  });
  vm.runInContext(source,context);
  assert.equal(vm.runInContext("selectLabel('discAppearance','vinyl')",context),'Vinyl');
  assert.equal(vm.runInContext("selectLabel('idleAppearance','vinyl')",context),'Vinyl standby');
  assert.equal(vm.runInContext("selectLabel('discAppearance','vinyl-cover')",context),'Vinyl & Cover');
  assert.equal(vm.runInContext("sections.find(s=>s.title==='Disc').fields[0][0]",context),'discAppearance');
  assert.equal(vm.runInContext("sections.find(s=>s.title==='Standby').fields[0][0]",context),'idleAppearance');
});

test('fresh state without a track enters standby',()=>{
  assert.equal(isEmptyPlayback({type:'state',state:{item:null,status:'paused'}}),true);
  assert.equal(isEmptyPlayback({item:null,status:'stopped'}),true);
});
test('a stopped playback event enters standby without an item field',()=>{
  assert.equal(isEmptyPlayback({type:'playback_changed',status:'stopped'}),true);
  assert.equal(isEmptyPlayback({type:'playback_changed',status:{state:'idle'}}),true);
});
test('pausing does not remove the artwork',()=>{
  assert.equal(isEmptyPlayback({type:'playback_changed',status:'paused'}),false);
  assert.equal(isEmptyPlayback({type:'position_changed',position:1500}),false);
});
test('a track wins over an inconsistent empty outer field',()=>{
  assert.equal(isEmptyPlayback({item:null,playback:{item:{uri:'spotify:track:a'},status:'playing'}}),false);
  assert.equal(isEmptyPlayback({item:{uri:'spotify:track:b'},status:'stopped'}),false);
});

test('pause delay supports any whole second up to a day, or never',()=>{
  assert.equal(pauseDelayMs(-1),null);
  assert.equal(pauseDelayMs(undefined),null);
  assert.equal(pauseDelayMs(0),0);
  assert.equal(pauseDelayMs(1),1000);
  assert.equal(pauseDelayMs(42),42000);
  assert.equal(pauseDelayMs(90),90000);
  assert.equal(pauseDelayMs(300),300000);
  assert.equal(pauseDelayMs(3600),3600000);
  assert.equal(pauseDelayMs(86400),86400000);
  assert.equal(pauseDelayMs(86401),null);
  assert.equal(pauseDelayMs(-2),null);
  assert.equal(pauseDelayMs(1.5),null);
  assert.equal(pauseDelayMs('300'),null);
});

import {DiscDirector} from '../src/motion.js';

function element(name,log){
  const classes=new Set();
  const nodes={};
  return {
    name,
    style:{setProperty(){}},
    dataset:{},
    offsetWidth:1,
    classList:{
      add(value){classes.add(value)},
      remove(value){classes.delete(value)},
      contains(value){return classes.has(value)}
    },
    querySelector(selector){return nodes[selector]??=element(name+selector,log)},
    animate(frames){
      log.push({name,frames});
      return {cancel(){}};
    }
  };
}

test('first song animates only the arriving disc in every appearance',async()=>{
  const originalDoc=globalThis.document;
  const originalFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const appearance of ['basic','cd','vinyl','vinyl-cover','cd-cover','album-cover']){
      for(const style of ['fly','slide','fade','off']){
        const log=[];
        const discs=[element('disc-a',log),element('disc-b',log)];
        const backdrops=[element('backdrop-a',log),element('backdrop-b',log)];
        const info=element('info',log);
        globalThis.document={
          documentElement:{dataset:{discAppearance:appearance}},
          querySelectorAll:()=>[]
        };
        const director=new DiscDirector({
          discs,backdrops,info,
          settings:{transitionStyle:style,transitionDuration:1,transitionDelay:0,
            sleeveTransitionStyle:'in-out',sleeveDuration:1,sleeveDelayIn:0,sleeveDelayOut:0}
        });
        let shown=0;
        await director.change('',()=>{shown++});
        assert.equal(shown,1,appearance+':'+style);
        assert.equal(director.started,true);
        assert.equal(discs.filter(d=>d.classList.contains('active')).length,1);
        assert.equal(log.length,style==='off'?0:1,appearance+':'+style);
        assert.ok(log.every(entry=>entry.name==='disc-a'),appearance+':'+style);

        director.reset();
        assert.equal(director.started,false);
        assert.ok(discs.every(d=>!d.classList.contains('active')));
        assert.ok(backdrops.every(b=>!b.classList.contains('active')));
        log.length=0;
        await director.change('',()=>{shown++});
        assert.equal(shown,2);
        assert.equal(log.length,style==='off'?0:1);
        assert.ok(log.every(entry=>entry.name==='disc-a'));
      }
    }
  }finally{
    globalThis.document=originalDoc;
    globalThis.requestAnimationFrame=originalFrame;
  }
});

test('cover playback retracts and resumes with the selected sleeve alignment',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const appearance of ['vinyl-cover','cd-cover']){
      for(const alignment of ['left','center']){
        for(const reducedMotion of [false,true]){
          const discs=[element('disc-a',[]),element('disc-b',[])];
          globalThis.document={documentElement:{dataset:{discAppearance:appearance}}};
          const director=new DiscDirector({
            discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
            settings:{pauseRetractMs:0,sleeveAlignment:alignment,reducedMotion}
          });
          director.started=true;
          const record=discs[0].querySelector('.record-track');
          const cover=discs[0].querySelector('.record-sleeve');
          record.style.transform='translateX(50%)';
          director.setPlaying(false);
          assert.equal(record.style.transform,'translateX(0)',appearance+':'+alignment);
          assert.equal(record.style.translate,alignment==='center'?'25% 0':'0% 0');
          assert.equal(cover.style.translate,alignment==='center'?'25% 0':'0% 0');
          assert.equal(cover.style.transform,undefined);
          assert.equal(record.style.transition,reducedMotion?'none':'transform 450ms ease-in-out, translate 450ms ease-in-out');
          assert.equal(cover.style.transition,reducedMotion?'none':'translate 450ms ease-in-out');
          director.setPlaying(true);
          assert.equal(record.style.transform,'translateX(50%)');
          assert.equal(record.style.translate,'0% 0');
          assert.equal(cover.style.translate,'0% 0');
          director.reset();
          assert.equal(director.pauseRetractTimer,null);
          assert.equal(director.pauseRetracted,false);
          assert.equal(cover.style.transition,'');
        }
      }
    }
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('pause retraction is delayed, cancellable, and independent of plain discs',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  const previousNow=Date.now;
  globalThis.requestAnimationFrame=()=>0;
  try{
    let now=1000;
    Date.now=()=>now;
    for(const appearance of ['vinyl-cover','cd-cover','vinyl','cd','basic']){
      globalThis.document={documentElement:{dataset:{discAppearance:appearance}}};
      const discs=[element('disc-a',[]),element('disc-b',[])];
      const director=new DiscDirector({
        discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
        settings:{pauseRetractMs:5000}
      });
      director.started=true;
      const track=discs[0].querySelector('.record-track');
      track.style.transform='translateX(50%)';
      director.setPlaying(false);
      assert.equal(track.style.transform,'translateX(50%)');
      now+=5001;
      director.schedulePauseRetraction();
      assert.equal(track.style.transform,['vinyl-cover','cd-cover'].includes(appearance)?'translateX(0)':'translateX(50%)');
      director.setPlaying(true);
      assert.equal(track.style.transform,'translateX(50%)');
      director.reset();
    }
    globalThis.document={documentElement:{dataset:{discAppearance:'cd-cover'}}};
    const discs=[element('disc-a',[]),element('disc-b',[])];
    const director=new DiscDirector({
      discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
      settings:{pauseRetractMs:0}
    });
    director.started=true;
    director.artworkChanging=true;
    director.setPlaying(false);
    assert.equal(director.pauseRetracted,false);
    director.artworkChanging=false;
    director.schedulePauseRetraction();
    assert.equal(director.pauseRetracted,true);
    director.reset();
  }finally{
    Date.now=previousNow;
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('standby markup matches the standalone preview and has no decorative status indicators',()=>{
  const {readFileSync}=fs;
  const app=readFileSync(new URL('../index.html',import.meta.url),'utf8');
  const preview=readFileSync(new URL('../docs/idle-preview.html',import.meta.url),'utf8');
  const standby=source=>source.match(/<section class="standby"[^]*?<\/section>/)?.[0];
  assert.ok(standby(app));
  assert.equal(standby(app),standby(preview));
  assert.ok(!standby(app).includes('standby-indicator'));
  assert.ok(!standby(app).includes('standby-pin'));
  assert.ok(!standby(app).includes('standby-description'));
  assert.ok(!standby(app).includes('standby-second'));
  assert.ok(!standby(app).includes('>S<'));
});

test('default standby message is blank in config, display and preview',()=>{
  const config=JSON.parse(fs.readFileSync(new URL('../config.json',import.meta.url),'utf8'));
  const main=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const app=fs.readFileSync(new URL('../index.html',import.meta.url),'utf8');
  const preview=fs.readFileSync(new URL('../docs/idle-preview.html',import.meta.url),'utf8');
  assert.equal(config.idleText,'');
  assert.match(main,/config\\.idleText\\.slice\\(0,80\\):''/);
  for(const html of [app,preview])assert.match(html,/<h1 class="standby-caption" id="standby-title"><\\/h1>/);
  assert.match(preview,/<input id="caption" type="text" maxlength="80" value="">/);
});

test('standby stays hidden until the first playback state arrives',()=>{
  const html=fs.readFileSync(new URL('../index.html',import.meta.url),'utf8');
  const css=fs.readFileSync(new URL('../styles.css',import.meta.url),'utf8');
  assert.match(html,/<html[^>]*data-display-state="connecting"/);
  assert.ok(css.includes(":root[data-display-state='connecting'] .standby,:root[data-display-state='playing'] .standby{visibility:hidden;opacity:0;pointer-events:none}"));
});

test('standby has flat settings background and no redundant connection copy',()=>{
  const app=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const style=fs.readFileSync(new URL('../styles.css',import.meta.url),'utf8');
  assert.match(style,/\.standby\{[^}]*background:#101010/);
  assert.match(style,/\.standby\{[^}]*font-family:Arial,Helvetica,sans-serif/);
  assert.match(style,/\.standby-clock\{[^}]*font-weight:600/);
  assert.match(style,/\.standby-record\{[^}]*width:min\(90vw,calc\(100dvh - 7rem\)\)/);
  assert.doesNotMatch(app,/Play something on Spotify to begin/);
  assert.doesNotMatch(app,/'PAUSED'|'READY'|standby-status|standby-description/);
  assert.match(app,/state==='offline'\?'Disconnected':state==='reconnecting'\?'Reconnecting'/);
});
