import test from 'node:test';
import assert from 'node:assert/strict';
import {dominantColourFromPixels} from '../src/backdrop-colour.mjs';

function artwork(width,height,pixel){
  const data=new Uint8ClampedArray(width*height*4);
  for(let y=0;y<height;y++)for(let x=0;x<width;x++){
    const [r,g,b,a=255]=pixel(x,y);
    data.set([r,g,b,a],(y*width+x)*4);
  }
  return {width,height,data};
}
const channels=hex=>[1,3,5].map(i=>parseInt(hex.slice(i,i+2),16));

test('dominant region wins over a small, striking accent and white lettering',()=>{
  const image=artwork(64,64,(x,y)=>y<6?[250,248,244]:x<49?[158,36,47]:[15,80,250]);
  const colour=dominantColourFromPixels(image);
  const [r,g,b]=channels(colour);
  assert.ok(r>g*1.4&&r>b*1.1,colour);
});
test('picks the vivid blue cover rather than averaging it into dull gray',()=>{
  const image=artwork(64,64,(x,y)=>x<44?[20,65,175]:y<50?[239,240,240]:[210,155,95]);
  const colour=dominantColourFromPixels(image);
  const [r,g,b]=channels(colour);
  assert.ok(b>r*1.25&&b>g*1.1,colour);
  assert.ok(Math.max(r,g,b)-Math.min(r,g,b)>35,colour);
});
test('monochrome covers have a restrained neutral fallback',()=>{
  const image=artwork(32,32,(x,y)=>(x+y)%2?[15,15,15]:[248,248,248]);
  const colour=dominantColourFromPixels(image);
  const values=channels(colour);
  assert.match(colour,/^#[0-9a-f]{6}$/);
  assert.ok(Math.max(...values)-Math.min(...values)<35,colour);
});
test('transparent artwork does not produce invalid colours',()=>{
  assert.equal(dominantColourFromPixels(artwork(8,8,()=>[0,0,0,0])),null);
});
test('highly saturated colours are gamut-mapped to valid six-digit hex',()=>{
  for(const rgb of [[255,0,0],[0,255,0],[0,0,255],[255,0,255],[255,230,0]]){
    assert.match(dominantColourFromPixels(artwork(12,12,()=>rgb)),/^#[0-9a-f]{6}$/);
  }
});
