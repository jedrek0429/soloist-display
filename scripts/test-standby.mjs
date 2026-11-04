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
