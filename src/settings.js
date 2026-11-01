const sections=[
  ['Audio visualiser',[
    ['visualizer','Enabled','toggle'],['visualizerSensitivity','Sensitivity','range',0,10,.1],['visualizerSmoothing','Smoothing','range',0,1,.05]
  ]],
  ['Disc & motion',[
    ['discSize','Disc diameter (dvh)','range',50,200,1],['discRotationSeconds','Seconds per revolution','range',1,120,1],
    ['animationSpeed','Animation speed','range',.1,5,.1],['discAnimation','Flying disc animations','toggle'],
    ['metadataAnimation','Metadata animations','toggle'],['reducedMotion','Reduced motion','toggle']
  ]],
  ['Backdrop & metadata',[
    ['backgroundDim','Background darkness','range',0,1,.01],['backgroundBlur','Background blur (px)','range',0,150,1],
    ['backgroundFadeDuration','Crossfade (ms)','range',0,10000,50],['metadataOpacity','Metadata opacity','range',0,1,.01],
    ['metadataScale','Metadata scale','range',.5,2,.05]
  ]],
  ['Connections',[
    ['soloistWebSocket','Soloist WebSocket','text'],['audioWebSocket','Audio WebSocket','text']
  ]]
];
let initial={};let defaults={};
const controls=document.querySelector('#controls'),status=document.querySelector('#status');
function render(values){
  controls.replaceChildren();
  for(const [title,fields] of sections){
    const section=document.createElement('section'),heading=document.createElement('h2');heading.textContent=title;section.append(heading);
    for(const [key,label,type,min,max,step] of fields){
      const line=document.createElement('div');line.className='field';
      const caption=document.createElement('label');caption.htmlFor=key;caption.textContent=label;
      const input=document.createElement('input');input.id=key;input.dataset.key=key;
      if(type==='toggle'){input.type='checkbox';input.checked=values[key]===true}
      else if(type==='range'){input.type='range';input.min=min;input.max=max;input.step=step;input.value=values[key]??min}
      else{input.type='text';input.value=values[key]??''}
      const output=document.createElement('output');output.htmlFor=key;
      const refresh=()=>{output.textContent=type==='toggle'?(input.checked?'On':'Off'):input.value};
      input.addEventListener('input',refresh);refresh();line.append(caption,input,output);section.append(line);
    }
    controls.append(section);
  }
}
function values(){
  const result={...initial};
  for(const input of controls.querySelectorAll('[data-key]')){
    result[input.dataset.key]=input.type==='checkbox'?input.checked:input.type==='range'?Number(input.value):input.value;
  }
  return result;
}
async function load(){
  const response=await fetch('/config.json',{cache:'no-store'});
  if(!response.ok)throw Error('Cannot load settings');
  initial=await response.json();render(initial);
}
document.querySelector('#save').addEventListener('click',async()=>{
  try{
    const response=await fetch('/config.json',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(values())});
    if(!response.ok)throw Error(await response.text());
    initial=await response.json();status.textContent='Saved. The display will apply the changes automatically.';
  }catch(error){status.textContent='Save failed: '+error.message}
});
document.querySelector('#reset').addEventListener('click',async()=>{
  if(!confirm('Reset display settings to defaults?'))return;
  try{
    const response=await fetch('/defaults.json',{cache:'no-store'});
    if(!response.ok)throw Error('Cannot load defaults');
    const values=await response.json();
    const save=await fetch('/config.json',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(values)});
    if(!save.ok)throw Error(await save.text());
    initial=await save.json();render(initial);status.textContent='Defaults restored.';
  }catch(error){status.textContent='Reset failed: '+error.message}
});
load().catch(error=>{status.textContent=error.message});
