import test from 'node:test';
import assert from 'node:assert/strict';
import {isEmptyPlayback,pauseDelayMs} from '../src/soloist.js';

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

test('pause delays are bounded and never is the default',()=>{
  assert.equal(pauseDelayMs('never'),null);
  assert.equal(pauseDelayMs(undefined),null);
  assert.equal(pauseDelayMs('0'),0);
  assert.equal(pauseDelayMs('30'),30_000);
  assert.equal(pauseDelayMs('60'),60_000);
  assert.equal(pauseDelayMs('300'),300_000);
  assert.equal(pauseDelayMs('600'),600_000);
  assert.equal(pauseDelayMs('3600'),null);
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
