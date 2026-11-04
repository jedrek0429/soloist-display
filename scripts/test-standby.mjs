import test from 'node:test';
import assert from 'node:assert/strict';
import {isEmptyPlayback} from '../src/soloist.js';

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
