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


test('standby fade is adjustable in Motion, including Pure black',()=>{
  const config=JSON.parse(fs.readFileSync(new URL('../config.json',import.meta.url),'utf8'));
  const settings=fs.readFileSync(new URL('../src/settings.js',import.meta.url),'utf8');
  const main=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const css=fs.readFileSync(new URL('../styles.css',import.meta.url),'utf8');
  assert.equal(config.idleTransitionStyle,'fade');
  assert.equal(config.idleFadeDuration,300);
  assert.match(settings,/title:'Idle motion'/);
  assert.match(settings,/\['idleTransitionStyle','Transition','select',\['fade','off'\]\]/);
  assert.match(settings,/key==='idleFadeDuration'&&draft\.idleTransitionStyle==='off'/);
  assert.match(settings,/'idleTransitionStyle','idleAppearance'\]\.includes\(key\)/);
  assert.match(settings,/\['idleFadeDuration','Duration','range',0,10000,50,'ms'\]/);
  assert.match(main,/root\.dataset\.idleTransition=config\.idleTransitionStyle==='off'\?'off':'fade'/);
  assert.match(main,/--idle-fade-duration':bounded\(config\.idleFadeDuration,300,0,10000\)\+'ms'/);
  assert.match(css,/\.standby\{[^}]*transition:opacity var\(--idle-fade-duration,300ms\) ease,visibility var\(--idle-fade-duration,300ms\)/);
  assert.match(css,/:root\[data-idle-appearance='black'\] \.standby\{background:#000\}/);
  assert.doesNotMatch(css,/:root\[data-idle-appearance='black'\] \.standby\{transition:none\}/);
  assert.match(css,/:root\[data-idle-transition='off'\] \.standby\{transition:none\}/);
  assert.match(css,/:root\[data-reduced-motion='true'\] \.standby[^\n]*transition:none!important/);
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
            settings:{pauseRetractMs:0,sleeveAlignment:alignment,reducedMotion,sleeveDuration:1200}
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
          assert.equal(record.style.transition,reducedMotion?'none':'transform 600ms ease-in-out, translate 600ms ease-in-out');
          assert.equal(cover.style.transition,reducedMotion?'none':'translate 600ms ease-in-out');
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

test('pause sleeve obeys the existing transition toggle and half-duration',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const appearance of ['vinyl-cover','cd-cover']){
      for(const style of ['in-out','off']){
        globalThis.document={documentElement:{dataset:{discAppearance:appearance}}};
        const discs=[element('disc-a',[]),element('disc-b',[])];
        const director=new DiscDirector({
          discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
          settings:{pauseRetractMs:0,sleeveTransitionStyle:style,sleeveDuration:1700}
        });
        director.started=true;
        director.setPlaying(false);
        const record=discs[0].querySelector('.record-track');
        const cover=discs[0].querySelector('.record-sleeve');
        const expected=style==='off'?'none':'transform 850ms ease-in-out, translate 850ms ease-in-out';
        assert.equal(record.style.transition,expected,appearance+':'+style);
        assert.equal(cover.style.transition,style==='off'?'none':'translate 850ms ease-in-out');
        director.setPlaying(true);
        assert.equal(record.style.transition,expected);
        director.reset();
      }
    }
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});


test('panel hides and returns on pause with its existing transition style and half-duration',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const style of ['slide','fade','off']){
      for(const reducedMotion of [false,true]){
        globalThis.document={documentElement:{dataset:{discAppearance:'basic'}}};
        const panel=element('info',[]);
        const director=new DiscDirector({
          discs:[element('disc-a',[]),element('disc-b',[])],
          backdrops:[element('bg-a',[]),element('bg-b',[])],info:panel,
          settings:{panelHideMs:0,panelTransitionStyle:style,panelTransitionDuration:1500,reducedMotion}
        });
        director.started=true;
        director.setPlaying(false);
        const animated=!reducedMotion&&style!=='off';
        assert.equal(director.panelPauseHidden,true);
        assert.equal(panel.style.transition,animated?`${style==='fade'?'opacity':'transform'} 750ms ease-in-out`:'none');
        assert.equal(panel.style.transform,style==='slide'?'translateY(calc(100% + 6vh + 60px))':'');
        assert.equal(panel.style.opacity,style==='slide'?'':'0');
        assert.equal(panel.style.pointerEvents,'none');
        director.setPlaying(true);
        assert.equal(director.panelPauseHidden,false);
        assert.equal(panel.style.transform,'');
        assert.equal(panel.style.opacity,'');
        assert.equal(panel.style.pointerEvents,'');
        assert.equal(panel.style.transition,animated?`${style==='fade'?'opacity':'transform'} 750ms ease-in-out`:'none');
        director.setPlaying(false);
        director.setPlaying(true,{instantResume:true});
        assert.equal(panel.style.transition,'none');
        assert.equal(panel.style.opacity,'');
        director.reset();
        assert.equal(panel.style.transition,'');
        assert.equal(director.panelPauseTimer,null);
      }
    }
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('panel pause delay is cancellable, measured from pause, and independent of disc appearance',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame,previousNow=Date.now;
  globalThis.requestAnimationFrame=()=>0;
  try{
    let now=1000;
    Date.now=()=>now;
    for(const appearance of ['basic','cd','vinyl-cover','cd-cover','album-cover']){
      globalThis.document={documentElement:{dataset:{discAppearance:appearance}}};
      const panel=element('info',[]);
      const director=new DiscDirector({
        discs:[element('disc-a',[]),element('disc-b',[])],
        backdrops:[element('bg-a',[]),element('bg-b',[])],info:panel,
        settings:{panelHideMs:5000,panelTransitionStyle:'fade',panelTransitionDuration:800}
      });
      director.started=true;
      director.setPlaying(false);
      assert.equal(director.panelPauseHidden,false);
      now+=3000;
      director.setPlaying(false);
      assert.equal(director.panelPauseStartedAt,now-3000);
      now+=2001;
      director.schedulePanelHide();
      assert.equal(director.panelPauseHidden,true);
      assert.equal(panel.style.opacity,'0');
      director.setPlaying(true);
      assert.equal(director.panelPauseTimer,null);
      assert.equal(director.panelPauseStartedAt,null);
      assert.equal(director.panelPauseHidden,false);
      director.reset();
    }
  }finally{
    Date.now=previousNow;
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('panel stays hidden on a paused track change, but reveals immediately from idle',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    globalThis.document={documentElement:{dataset:{discAppearance:'vinyl'}}};
    const log=[],panel=element('info',log);
    const director=new DiscDirector({
      discs:[element('disc-a',[]),element('disc-b',[])],
      backdrops:[element('bg-a',[]),element('bg-b',[])],info:panel,
      settings:{panelHideMs:0,panelTransitionStyle:'slide',panelTransitionDuration:1200}
    });
    director.started=true;
    director.setPlaying(false);
    let updated=false;
    director.changePanel(()=>{updated=true});
    assert.equal(updated,true);
    assert.equal(director.panelPauseHidden,true);
    assert.equal(panel.style.transform,'translateY(calc(100% + 6vh + 60px))');
    assert.equal(log.length,0,'no panel transition may unhide a paused panel');
    director.setPlaying(true,{instantResume:true});
    assert.equal(panel.style.transform,'');
    assert.equal(panel.style.transition,'none');
    director.reset();
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('pause while panel metadata is animating commits the latest text before hiding',async()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    globalThis.document={documentElement:{dataset:{discAppearance:'vinyl'}}};
    const panel=element('info',[]);
    panel.animate=()=>{
      const animation={oncancel:null,cancel(){this.oncancel?.()}};
      return animation;
    };
    const director=new DiscDirector({
      discs:[element('disc-a',[]),element('disc-b',[])],
      backdrops:[element('bg-a',[]),element('bg-b',[])],info:panel,
      settings:{panelHideMs:0,panelTransitionStyle:'fade',panelTransitionDuration:1200}
    });
    director.started=true;
    let updates=0;
    director.changePanel(()=>updates++);
    assert.equal(updates,0,'panel text waits for the exit transition');
    director.setPlaying(false);
    assert.equal(updates,1,'hide must not discard a pending metadata update');
    assert.equal(director.panelPauseHidden,true);
    await Promise.resolve();
    assert.equal(updates,1);
    director.reset();
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('never and a disabled panel do not schedule pause hiding',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    globalThis.document={documentElement:{dataset:{discAppearance:'basic'}}};
    for(const settings of [{panelHideMs:null},{panelHideMs:0,panelEnabled:false}]){
      const panel=element('info',[]);
      const director=new DiscDirector({
        discs:[element('disc-a',[]),element('disc-b',[])],
        backdrops:[element('bg-a',[]),element('bg-b',[])],info:panel,
        settings
      });
      director.started=true;
      director.setPlaying(false);
      assert.equal(director.panelPauseHidden,false);
      assert.equal(director.panelPauseTimer,null);
      director.reset();
    }
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('changing a retracted cover leaves it centred and skips the retract wait',async()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  const previousTimeout=globalThis.setTimeout;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const appearance of ['vinyl-cover','cd-cover']){
      for(const alignment of ['center','left']){
        const delays=[],log=[];
        globalThis.setTimeout=(callback,ms=0,...args)=>{
          delays.push(ms);
          queueMicrotask(()=>callback(...args));
          return 1;
        };
        globalThis.document={
          documentElement:{dataset:{discAppearance:appearance}},
          querySelectorAll:()=>[]
        };
        const discs=[element('disc-a',log),element('disc-b',log)];
        const backdrops=[element('bg-a',log),element('bg-b',log)];
        const director=new DiscDirector({
          discs,backdrops,info:element('info',log),
          settings:{
            pauseRetractMs:0,sleeveAlignment:alignment,sleeveTransitionStyle:'in-out',
            sleeveDelayIn:200,sleeveDelayOut:0,sleeveDuration:2,
            transitionStyle:'fly',transitionDelay:300,transitionDuration:2,
            backdropTransitionStyle:'off',panelEnabled:false
          }
        });
        director.started=true;
        discs[0].classList.add('active');
        backdrops[0].classList.add('active');
        director.setPlaying(false);
        const track=discs[0].querySelector('.record-track');
        const sleeve=discs[0].querySelector('.record-sleeve');
        const offset=alignment==='center'?'25% 0':'0% 0';
        assert.equal(track.style.translate,offset);
        assert.equal(sleeve.style.translate,offset);

        let departure;
        const animate=discs[0].animate;
        discs[0].animate=(frames,options)=>{
          departure={track:track.style.translate,sleeve:sleeve.style.translate};
          return animate(frames,options);
        };
        await director.change('',()=>{});
        assert.deepEqual(departure,{track:offset,sleeve:offset},appearance+':'+alignment);
        assert.ok(delays.every(ms=>ms<100),appearance+':'+alignment+': '+delays);
        assert.equal(director.current,1);
        assert.equal(director.artworkChanging,false);
      }
    }
  }finally{
    globalThis.setTimeout=previousTimeout;
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('returning from idle reveals the fully extended sleeve without a late slide',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    for(const appearance of ['vinyl-cover','cd-cover']){
      for(const alignment of ['left','center']){
        globalThis.document={documentElement:{dataset:{discAppearance:appearance}}};
        const discs=[element('disc-a',[]),element('disc-b',[])];
        const director=new DiscDirector({
          discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
          settings:{pauseRetractMs:0,sleeveAlignment:alignment}
        });
        director.started=true;
        const track=discs[0].querySelector('.record-track');
        const sleeve=discs[0].querySelector('.record-sleeve');
        track.style.transform='translateX(50%)';
        director.setPlaying(false);
        assert.equal(director.pauseRetracted,true);
        director.setPlaying(true,{instantResume:true});
        assert.equal(track.style.transform,'translateX(50%)');
        assert.equal(track.style.translate,'0% 0');
        assert.equal(sleeve.style.translate,'0% 0');
        assert.equal(track.style.transition,'none');
        assert.equal(sleeve.style.transition,'none');

        // Normal visible pauses must still animate in both directions.
        director.setPlaying(false);
        assert.equal(track.style.transition,'transform 500ms ease-in-out, translate 500ms ease-in-out');
        director.setPlaying(true);
        assert.equal(track.style.transition,'transform 500ms ease-in-out, translate 500ms ease-in-out');
        director.reset();
      }
    }
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('a short pause returns from idle without retracting the sleeve',()=>{
  const previousDoc=globalThis.document,previousFrame=globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame=()=>0;
  try{
    globalThis.document={documentElement:{dataset:{discAppearance:'vinyl-cover'}}};
    const discs=[element('disc-a',[]),element('disc-b',[])];
    const director=new DiscDirector({
      discs,backdrops:[element('bg-a',[]),element('bg-b',[])],info:element('info',[]),
      settings:{pauseRetractMs:5000}
    });
    director.started=true;
    const track=discs[0].querySelector('.record-track');
    track.style.transform='translateX(50%)';
    director.setPlaying(false);
    assert.equal(director.pauseRetracted,false);
    director.setPlaying(true,{instantResume:true});
    assert.equal(track.style.transform,'translateX(50%)');
    assert.equal(director.pauseRetractTimer,null);
    director.reset();
  }finally{
    globalThis.document=previousDoc;
    globalThis.requestAnimationFrame=previousFrame;
  }
});

test('standby state is checked before the playing scene is revealed',()=>{
  const source=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const call="director.setPlaying(playing,{instantResume:playing&&root.dataset.displayState==='paused'})";
  assert.ok(source.includes(call));
  assert.ok(source.indexOf(call)<source.indexOf("if(currentUri)setDisplayState('playing')"));
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
  assert.ok(main.includes("config.idleText.slice(0,80):''"));
  for(const html of [app,preview])assert.ok(html.includes('<h1 class="standby-caption" id="standby-title"></h1>'));
  assert.match(preview,/<input id="caption" type="text" maxlength="80" value="">/);
});

test('boot hides standby briefly; a later reconnect keeps idle visible',()=>{
  const html=fs.readFileSync(new URL('../index.html',import.meta.url),'utf8');
  const css=fs.readFileSync(new URL('../styles.css',import.meta.url),'utf8');
  const main=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  assert.match(html,/<html[^>]*data-display-state="connecting" data-initializing="true"/);
  assert.ok(css.includes(":root[data-display-state='connecting'][data-initializing='true'] .standby,:root[data-display-state='playing'] .standby{visibility:hidden;opacity:0;pointer-events:none}"));
  assert.match(main,/if\(state!=='connecting'\)root\.dataset\.initializing='false'/);
  assert.match(main,/if\(!currentUri\)setDisplayState\('connecting'\)/);
  assert.match(main,/if\(!currentUri&&s\.readyState===WebSocket\.OPEN&&root\.dataset\.displayState==='connecting'\)setDisplayState\('ready'\)/);
});

test('connected standby notice survives the empty state and expires after two seconds',()=>{
  const main=fs.readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
  const start=main.indexOf('let connectionNoticeTimer=null,connectedNoticePending=false;');
  const end=main.indexOf('let currentUri=null,',start);
  assert.ok(start!==-1&&end>start);
  const root={dataset:{displayState:'connecting',initializing:'true'}};
  const notice={textContent:''};
  const timers=new Map();
  let next=1;
  const context=vm.createContext({
    root,$:()=>notice,
    setTimeout:(fn,ms)=>{const id=next++;timers.set(id,{fn,ms});return id;},
    clearTimeout:id=>timers.delete(id)
  });
  vm.runInContext(main.slice(start,end),context);
  const set=state=>vm.runInContext("setDisplayState('"+state+"')",context);
  set('connecting');
  assert.equal(notice.textContent,'Connecting...');
  set('offline');
  assert.equal(root.dataset.initializing,'false');
  assert.equal(notice.textContent,'Disconnected');
  set('reconnecting');
  assert.equal(notice.textContent,'Reconnecting...');
  set('connecting'); // After the initial attempt this no longer hides the standby UI.
  assert.equal(notice.textContent,'Connecting...');
  vm.runInContext('connectedNoticePending=true',context);
  set('ready');
  assert.equal(notice.textContent,'Connected!');
  assert.equal(root.dataset.displayState,'ready');
  assert.equal([...timers.values()][0].ms,2000);
  set('ready'); // An empty get_state result must not erase the notice.
  assert.equal(notice.textContent,'Connected!');
  const [timerId,timer]=timers.entries().next().value;
  timers.delete(timerId); // Simulate the timer being consumed by the event loop.
  timer.fn();
  assert.equal(notice.textContent,'');
  vm.runInContext('connectedNoticePending=true',context);
  set('ready');
  set('playing'); // Music takes priority over the notice.
  assert.equal(notice.textContent,'');
  assert.equal(timers.size,0);
  vm.runInContext('connectedNoticePending=true',context);
  set('offline'); // Losing the connection must not show a stale success message.
  assert.equal(notice.textContent,'Disconnected');
  assert.equal(timers.size,0);
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
  assert.match(app,/connecting:'Connecting\.\.\.',offline:'Disconnected',reconnecting:'Reconnecting\.\.\.'/);
});
