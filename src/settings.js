const sections=[
  {
    title:'Audio visualiser',
    kicker:'01 · Sound',
    description:'Shape the six-band spectrum drawn from your current audio output.',
    fields:[
      ['visualizer','Visualiser','toggle'],
      ['visualizerSensitivity','Sensitivity','range',0,10,.1],
      ['visualizerSmoothing','Smoothing','range',0,1,.05],
      ['visualizerAutoNormalize','Automatic normalisation','toggle'],
      ['visualizerPeakHold','Peak hold','toggle'],
      ['visualizerBarCount','Bars','select',[6,10,16]]
    ]
  },
  {
    title:'Disc & motion',
    kicker:'02 · Movement',
    description:'Control the record scale, rotation and track-change choreography.',
    fields:[
      ['discSize','Disc diameter','range',50,200,1,'dvh'],
      ['discRotationSeconds','Rotation speed','range',1,120,1,'sec'],
      ['animationSpeed','Transition speed','range',.1,5,.1,'×'],
      ['discAnimation','Disc exchange','toggle'],
      ['transitionStyle','Transition style','select',['fly','fade','instant']],
      ['discAppearance','Disc appearance','select',['dot','basic','cd','vinyl']],
      ['burnInProtection','Screen burn-in protection','toggle'],
      ['metadataAnimation','Metadata reveal','toggle'],
      ['reducedMotion','Reduced motion','toggle']
    ]
  },
  {
    title:'Backdrop & metadata',
    kicker:'03 · Picture',
    description:'Balance the album-art backdrop against readable track information.',
    fields:[
      ['backgroundDim','Backdrop darkness','range',0,1,.01],
      ['backgroundBlur','Backdrop blur','range',0,150,1,'px'],
      ['backgroundFadeDuration','Backdrop crossfade','range',0,10000,50,'ms'],
      ['metadataOpacity','Panel opacity','range',0,1,.01],
      ['metadataScale','Panel scale','range',.5,2,.05,'×']
    ]
  },
  {
    title:'Connections',
    kicker:'04 · Network',
    description:'Local WebSocket endpoints used for playback state and spectrum data.',
    fields:[
      ['soloistWebSocket','Soloist','text'],
      ['audioWebSocket','Audio bridge','text']
    ]
  }
];

const controls=document.querySelector('#controls');
const nav=document.querySelector('#section-nav');
const status=document.querySelector('#status');
let draft={};
let activeSection=0;

function format(value,suffix=''){
  const text=typeof value==='number'?String(Number(value.toFixed(2))):String(value);
  return suffix?text+' '+suffix:text;
}

function renderNav(){
  nav.querySelectorAll('.nav-button').forEach(button=>button.remove());
  sections.forEach((section,index)=>{
    const button=document.createElement('button');
    button.type='button';
    button.className='nav-button';
    button.setAttribute('aria-current',index===activeSection?'page':'false');
    const number=document.createElement('span');
    number.className='nav-index';
    number.textContent=String(index+1).padStart(2,'0');
    const title=document.createElement('span');
    title.className='nav-title';
    title.textContent=section.title;
    button.append(number,title);
    button.addEventListener('click',()=>{
      activeSection=index;
      renderNav();
      renderSection();
    });
    nav.append(button);
  });
}

function renderSection(){
  const section=sections[activeSection];
  document.querySelector('#panel-kicker').textContent=section.kicker;
  document.querySelector('#panel-title').textContent=section.title;
  document.querySelector('#panel-description').textContent=section.description;
  controls.replaceChildren();

  for(const [key,label,type,min,max,step,suffix] of section.fields){
    const line=document.createElement('div');
    line.className='field';
    const caption=document.createElement('label');
    caption.className='field-label';
    caption.htmlFor=key;
    caption.textContent=label;
    line.append(caption);

    const input=document.createElement('input');
    input.id=key;
    input.dataset.key=key;
    if(type==='toggle'){
      input.type='checkbox';
      input.checked=draft[key]===true;
      const wrapper=document.createElement('label');
      wrapper.className='switch';
      wrapper.htmlFor=key;
      const track=document.createElement('span');
      track.className='switch-track';
      wrapper.append(input,track);
      line.append(wrapper);
    }else if(type==='select'){
      const select=document.createElement('select');
      select.id=key;select.dataset.key=key;
      for(const choice of min){const option=document.createElement('option');option.value=String(choice);option.textContent=String(choice).replace(/^./,c=>c.toUpperCase());select.append(option)}
      select.value=String(draft[key]??min[0]);
      line.append(select);
      input.replaceWith?.();
      const output=document.createElement('output');output.htmlFor=key;
      const refresh=()=>{draft[key]=typeof min[0]==='number'?Number(select.value):select.value;output.textContent=String(draft[key]);};
      select.addEventListener('change',()=>{refresh();status.textContent='Unsaved changes'});
      refresh();line.append(output);controls.append(line);continue;
    }else if(type==='range'){
      input.type='range';
      input.min=min;
      input.max=max;
      input.step=step;
      input.value=draft[key]??min;
      line.append(input);
    }else{
      input.type='text';
      input.value=draft[key]??'';
      input.spellcheck=false;
      line.append(input);
    }

    const output=document.createElement('output');
    output.htmlFor=key;
    const refresh=()=>{
      const value=type==='toggle'?input.checked:type==='range'?Number(input.value):input.value;
      draft[key]=value;
      output.textContent=type==='toggle'?(value?'On':'Off'):type==='text'?'Local':format(value,suffix);
    };
    input.addEventListener('input',()=>{
      refresh();
      status.textContent='Unsaved changes';
    });
    refresh();
    line.append(output);
    controls.append(line);
  }
}

function render(){
  renderNav();
  renderSection();
}

async function load(){
  const response=await fetch('/config.json',{cache:'no-store'});
  if(!response.ok)throw Error('Cannot load settings');
  draft=await response.json();
  render();
}

document.querySelector('#save').addEventListener('click',async()=>{
  try{
    const response=await fetch('/config.json',{
      method:'PUT',
      headers:{'Content-Type':'application/json'},
      body:JSON.stringify(draft)
    });
    if(!response.ok)throw Error(await response.text());
    draft=await response.json();
    renderSection();
    status.textContent='Saved · display updates automatically';
  }catch(error){
    status.textContent='Save failed · '+error.message;
  }
});

document.querySelector('#reset').addEventListener('click',async()=>{
  if(!confirm('Reset display settings to defaults?'))return;
  try{
    const response=await fetch('/defaults.json',{cache:'no-store'});
    if(!response.ok)throw Error('Cannot load defaults');
    const defaults=await response.json();
    const save=await fetch('/config.json',{
      method:'PUT',
      headers:{'Content-Type':'application/json'},
      body:JSON.stringify(defaults)
    });
    if(!save.ok)throw Error(await save.text());
    draft=await save.json();
    render();
    status.textContent='Defaults restored';
  }catch(error){
    status.textContent='Reset failed · '+error.message;
  }
});

load().catch(error=>{status.textContent=error.message});
