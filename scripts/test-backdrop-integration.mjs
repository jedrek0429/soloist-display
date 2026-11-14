import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {DiscDirector} from '../src/motion.js';
import {dominantColourForArtwork} from '../src/backdrop-colour.mjs';

function element(){
  const classes=new Set(),children=new Map();
  return {
    style:{setProperty(){}},dataset:{},offsetWidth:1,
    classList:{add:key=>classes.add(key),remove:key=>classes.delete(key),contains:key=>classes.has(key)},
    querySelector:key=>{
      if(!children.has(key))children.set(key,element());
      return children.get(key);
    }
  };
}
function pixels(rgb){
  const data=new Uint8ClampedArray(64*64*4);
  for(let i=0;i<data.length;i+=4)data.set([...rgb,255],i);
  return {data,width:64,height:64};
}
async function scenario({mode,appearance='basic',failCors=false,colours={}}){
  const original={Image:globalThis.Image,document:globalThis.document,frame:globalThis.requestAnimationFrame};
  const requests=[],draws=[];
  let sampledImage;
  class FakeImage{
    set src(url){
      this.url=url;
      requests.push({url,cors:this.crossOrigin==='anonymous'});
      queueMicrotask(()=>{
        if(failCors&&this.crossOrigin==='anonymous')this.onerror?.();
        else this.onload?.();
      });
    }
  }
  globalThis.Image=FakeImage;
  globalThis.requestAnimationFrame=()=>0;
  globalThis.document={
    documentElement:{dataset:{discAppearance:appearance}},
    querySelectorAll:()=>[],
    createElement:tag=>{
      assert.equal(tag,'canvas');
      return {getContext:()=>({
        drawImage:image=>{sampledImage=image;draws.push(image.url)},
        getImageData:()=>pixels(colours[sampledImage.url]||[190,20,50])
      })};
    }
  };
  const backdrops=[element(),element()];
  const director=new DiscDirector({
    discs:[element(),element()],backdrops,info:element(),
    settings:{backgroundMode:mode,backgroundColor:'#be477a',
      reducedMotion:true,transitionStyle:'off',backdropTransitionStyle:'off'}
  });
  try{
    const shown=[];
    await director.change('integration-red.png',()=>shown.push('red'));
    const first=backdrops[director.current];
    const snapshot=background=>({
      kind:background.dataset.backdropKind,
      image:background.style.backgroundImage,
      colour:background.style.backgroundColor
    });
    const before=snapshot(first);
    await director.change('integration-blue.png',()=>shown.push('blue'));
    return {requests,draws,first:before,second:snapshot(backdrops[director.current]),shown};
  }finally{
    director.reset();
    globalThis.Image=original.Image;
    globalThis.document=original.document;
    globalThis.requestAnimationFrame=original.frame;
  }
}

test('default artwork mode uses the image without CORS or palette extraction',async()=>{
  const {first,requests,draws}=await scenario({mode:'blurred-artwork'});
  assert.equal(first.kind,'artwork');
  assert.match(first.image,/integration-red\.png/);
  assert.equal(first.colour,'');
  assert.deepEqual(requests.map(x=>x.cors),[false,false]);
  assert.equal(draws.length,0);
});
test('custom colour is uniform and does not sample the cover, including vinyl sleeve',async()=>{
  const {first,second,draws}=await scenario({mode:'custom-colour',appearance:'vinyl-cover'});
  for(const bg of [first,second]){
    assert.equal(bg.kind,'colour');
    assert.equal(bg.colour,'#be477a');
    assert.equal(bg.image,'none');
  }
  assert.equal(draws.length,0);
});
test('dominant mode samples each preloaded image exactly once',async()=>{
  const {first,second,requests,draws,shown}=await scenario({
    mode:'dominant-colour',appearance:'vinyl-cover',
    colours:{'integration-red.png':[190,23,49],'integration-blue.png':[35,45,205]}
  });
  assert.deepEqual(requests.map(x=>x.cors),[true,true]);
  assert.deepEqual(draws,['integration-red.png','integration-blue.png']);
  assert.equal(first.kind,'colour');
  assert.equal(second.kind,'colour');
  assert.notEqual(first.colour,second.colour);
  assert.deepEqual(shown,['red','blue']);
});
test('CORS failure falls back to each blurred artwork without a duplicate request',async()=>{
  const {first,second,requests,draws}=await scenario({mode:'dominant-colour',failCors:true});
  for(const bg of [first,second]){
    assert.equal(bg.kind,'artwork');
    assert.match(bg.image,/integration-/);
    assert.equal(bg.colour,'');
  }
  assert.equal(requests.length,2);
  assert.equal(draws.length,0);
});
test('tainted canvas pixel reads fail gracefully',()=>{
  const previous=globalThis.document;
  globalThis.document={createElement:()=>({getContext:()=>({
    drawImage(){throw new Error('Tainted canvas')}
  })})};
  try{
    assert.equal(dominantColourForArtwork('tainted-integration.png',{}),null);
  }finally{globalThis.document=previous}
});
test('CSS applies blur per image, including a dominant-colour fallback',()=>{
  const css=readFileSync(new URL('../styles.css',import.meta.url),'utf8');
  assert.match(css,/\.backdrop-image\[data-backdrop-kind='colour'\]\{inset:0;filter:none;transform:none\}/);
  assert.doesNotMatch(css,/data-background-mode='dominant-colour'\] \.backdrop-image/);
  assert.match(css,/\.backdrop-image\{[^}]*blur\(var\(--background-blur,50px\)\)/);
});
test('settings expose three modes and only their relevant controls',()=>{
  const defaults=JSON.parse(readFileSync(new URL('../config.json',import.meta.url),'utf8'));
  const src=readFileSync(new URL('../src/settings.js',import.meta.url),'utf8');
  assert.equal(defaults.backgroundMode,'blurred-artwork');
  assert.match(src,/\['backgroundMode','Type','select',\['blurred-artwork','dominant-colour','custom-colour'\]\]/);
  assert.match(src,/key==='backgroundBlur'&&draft\.backgroundMode!=='blurred-artwork'/);
  assert.match(src,/key==='backgroundColor'&&draft\.backgroundMode!=='custom-colour'/);
});
