const sections=[
  {
    title:'Disc',
    fields:[
      ['discAppearance','Appearance','select',['basic','cd','vinyl','vinyl-cover','cd-cover']],
      ['discSize','Diameter','range',50,200,1,'dvh'],
      ['centerHole','Center hole','range',0,20,.5,'%'],
      ['centerHoleColor','Hole colour','select',['white','black','transparent']],
      ['cdCoverSide','CD side','select',['up','down']],
      ['vinylGrooveDensity','Groove density','range',1,10,1]
    ]
  },
  {
    title:'Backdrop',
    fields:[
      ['backgroundDim','Darkness','range',0,1,.01],
      ['backgroundBlur','Blur','range',0,150,1,'px']
    ]
  },
  {
    title:'Panel',
    fields:[
      ['metadataOpacity','Opacity','range',0,1,.01],
      ['metadataScale','Scale','range',.5,2,.05,'×'],
      ['panelPosition','Position','select',['bottom-right','bottom-left','top-right','top-left']],
      ['panelFontSize','Font size','range',.6,1.6,.05,'×'],
      ['progressBar','Progress bar','toggle'],
      ['timeDisplay','Time display','select',['off','remaining','total']],
      ['timeLayout','Time layout','select',['split','together']]
    ]
  },
  {
    title:'Visualiser',
    fields:[
      ['visualizer','Enabled','toggle'],
      ['visualizerSensitivity','Sensitivity','range',0,10,.1],
      ['visualizerSmoothing','Smoothing','range',0,1,.05],
      ['visualizerAutoNormalize','Automatic normalisation','toggle'],
      ['visualizerPeakHold','Peak hold','toggle']
    ]
  },
  {
    title:'Motion',
    fields:[
      ['changeDiscOn','Change disc on','select',['song','album']],
      ['transitionStyle','Disc transition','select',['fly','slide','fade','off']],
      ['sleeveTransitionStyle','Sleeve animation','select',['in-out','off']],
      ['sleeveDuration','Sleeve duration','range',100,10000,50,'ms'],
      ['sleeveAlignment','Sleeve alignment','select',['left','center']],
      ['backdropTransitionStyle','Backdrop transition','select',['fade','off']],
      ['panelTransitionStyle','Panel transition','select',['slide','fade','off']],
      ['transitionDuration','Disc duration','range',100,10000,50,'ms'],
      ['backdropTransitionDuration','Backdrop duration','range',100,10000,50,'ms'],
      ['panelTransitionDuration','Panel duration','range',100,10000,50,'ms'],
      ['discRotationSeconds','Rotation speed','range',1,120,1,'sec'],
      ['reducedMotion','Reduced motion','toggle']
    ]
  },
  {
    title:'Display',
    fields:[
      ['burnInProtection','Screen burn-in protection','toggle']
    ]
  },
  {
    title:'Connections',
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
const isCover=style=>style==='vinyl-cover'||style==='cd-cover';
const holeKey=style=>style==='vinyl-cover'?'centerHoleVinyl':style==='cd-cover'?'centerHoleCd':'centerHole'+style[0].toUpperCase()+style.slice(1);
const selectedHoleKey=()=>holeKey(draft.discAppearance||'basic');
const selectedSizeKey=()=>selectedHoleKey().replace('centerHole','discSize');
const selectedHoleColorKey=()=>selectedHoleKey().replace('centerHole','centerHoleColor');

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
    const title=document.createElement('span');
    title.className='nav-title';
    title.textContent=section.title;
    button.append(title);
    button.addEventListener('click',()=>{
      activeSection=index;
      renderNav();
      renderSection();
    });
    nav.append(button);
  });
}

function renderSection(){
  const hidden=key=>(key==='cdCoverSide'&&draft.discAppearance!=='cd-cover')||
    (key==='vinylGrooveDensity'&&!['vinyl','vinyl-cover'].includes(draft.discAppearance))||
    (key==='transitionStyle'&&isCover(draft.discAppearance))||
    (key==='sleeveTransitionStyle'&&!isCover(draft.discAppearance))||
    (key==='sleeveDuration'&&(!isCover(draft.discAppearance)||draft.sleeveTransitionStyle==='off'))||
    (key==='sleeveAlignment'&&!isCover(draft.discAppearance))||
    (key==='timeLayout'&&draft.timeDisplay==='off')||
    (key==='transitionDuration'&&draft.transitionStyle==='off')||
    (key==='backdropTransitionDuration'&&draft.backdropTransitionStyle==='off')||
    (key==='panelTransitionDuration'&&draft.panelTransitionStyle==='off');
  const section=sections[activeSection];
  document.querySelector('#panel-kicker').textContent='';
  document.querySelector('#panel-title').textContent=section.title;
  document.querySelector('#panel-description').textContent='';
  controls.replaceChildren();

  for(const [key,label,type,min,max,step,suffix] of section.fields){
    const line=document.createElement('div');
    line.className='field';
    if(hidden(key))line.hidden=true;
    const caption=document.createElement('label');
    caption.className='field-label';
    caption.htmlFor=key;
    caption.textContent=label;
    line.append(caption);
    if(key==='transitionStyle')line.classList.add('transition-disc');
    if(key==='backdropTransitionStyle')line.classList.add('transition-backdrop');

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
      for(const choice of min){const option=document.createElement('option');option.value=String(choice);option.textContent=({cd:'CD','vinyl-cover':'Vinyl & Cover','cd-cover':'CD & Cover'}[choice]||String(choice).replace(/^./,c=>c.toUpperCase()));select.append(option)}
      select.value=String((key==='centerHoleColor'?draft[selectedHoleColorKey()]:draft[key])??min[0]);
      line.append(select);
      const output=document.createElement('output');output.htmlFor=key;
      const refresh=()=>{const value=typeof min[0]==='number'?Number(select.value):select.value;draft[key==='centerHoleColor'?selectedHoleColorKey():key]=value;output.textContent=String(value);};
      select.addEventListener('change',()=>{refresh();if(['discAppearance','timeDisplay','transitionStyle','sleeveTransitionStyle','backdropTransitionStyle','panelTransitionStyle'].includes(key))renderSection();status.textContent='Unsaved changes';});
      refresh();controls.append(line);continue;
    }else if(type==='range'){
      input.type='range';
      input.min=min;
      input.max=max;
      input.step=step;
      input.value=(key==='centerHole'?draft[selectedHoleKey()]:key==='discSize'?draft[selectedSizeKey()]:draft[key])??min;
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
      draft[key==='centerHole'?selectedHoleKey():key==='discSize'?selectedSizeKey():key]=value;
      output.textContent=type==='toggle'?(value?'On':'Off'):type==='text'?'Local':format(value,suffix);
    };
    input.addEventListener('input',()=>{
      refresh();
      status.textContent='Unsaved changes';
    });
    if(key!=='centerHole')refresh();else output.textContent=format(Number(input.value),suffix);
    if(type!=='toggle')line.append(output);
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
  draft.changeDiscOn=draft.changeDiscOn==='album'?'album':'song';
  delete draft.backdropLinked;
  for(const key of ['transitionStyle','backdropTransitionStyle','panelTransitionStyle'])if(draft[key]==='instant')draft[key]='off';
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
    draft.changeDiscOn=draft.changeDiscOn==='album'?'album':'song';
    delete draft.backdropLinked;
    for(const key of ['transitionStyle','backdropTransitionStyle','panelTransitionStyle'])if(draft[key]==='instant')draft[key]='off';
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
    delete draft.backdropLinked;
    render();
    status.textContent='Defaults restored';
  }catch(error){
    status.textContent='Reset failed · '+error.message;
  }
});

load().catch(error=>{status.textContent=error.message});
