const sections=[
  {
    title:'Disc',
    fields:[
      ['discAppearance','Appearance','select',['basic','cd','vinyl','vinyl-cover','cd-cover','album-cover']],
      ['discOpacity','Disc opacity','range',0,1,.05],
      ['discSize','Diameter','range',50,200,1,'dvh'],
      ['vinylLabelSize','Vinyl label size','range',10,70,1,'%'],
      ['centerHole','Center hole','range',0,20,.5,'%'],
      ['centerHoleColor','Hole colour','select',['white','black','transparent']],
      ['cdCoverSide','CD side','select',['up','down']],
      ['cdRainbow','Rainbow','range',0,1,.05],
      ['vinylGrooveDensity','Groove density','range',1,10,1],
      ['changeDiscOn','Change disc on','select',['song','album']],
      ['discRotationSeconds','Rotation speed','range',1,120,1,'sec'],
      ['reducedMotion','Reduced motion','toggle']
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
      ['panelEnabled','Enabled','toggle'],
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
    title:'Sleeve motion',
    fields:[
      ['sleeveTransitionStyle','Animation','select',['in-out','off']],
      ['sleeveAlignment','Alignment','select',['left','center']],
      ['sleeveDuration','Duration','range',100,10000,50,'ms'],
      ['sleeveDelayIn','Delay in','range',0,10000,50,'ms'],
      ['sleeveDelayOut','Delay out','range',0,10000,50,'ms']
    ]
  },
  {
    title:'Disc motion',
    fields:[
      ['transitionStyle','Transition','select',['fly','slide','fade','off']],
      ['transitionDuration','Duration','range',100,10000,50,'ms'],
      ['transitionDelay','Delay','range',0,10000,50,'ms']
    ]
  },
  {
    title:'Backdrop motion',
    fields:[
      ['backdropTransitionStyle','Transition','select',['fade','off']],
      ['backdropTransitionDuration','Duration','range',100,10000,50,'ms'],
      ['backdropTransitionDelay','Delay','range',0,10000,50,'ms']
    ]
  },
  {
    title:'Panel motion',
    fields:[
      ['panelTransitionStyle','Transition','select',['slide','fade','off']],
      ['panelTransitionDuration','Duration','range',100,10000,50,'ms'],
      ['panelDelayIn','Delay in','range',0,10000,50,'ms'],
      ['panelDelayOut','Delay out','range',0,10000,50,'ms']
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
let motionExpanded=false;
const motionPages=new Set(['Sleeve motion','Disc motion','Backdrop motion','Panel motion']);
const isCover=style=>style==='vinyl-cover'||style==='cd-cover';
const motionVisible=title=>title!=='Sleeve motion'||isCover(draft.discAppearance);
const motionLabel=title=>title==='Disc motion'&&draft.discAppearance==='album-cover'?'Album cover':title.replace(' motion','');
const holeKey=style=>['vinyl','vinyl-cover'].includes(style)?'centerHoleVinyl':['cd','cd-cover'].includes(style)?'centerHoleCd':'centerHoleBasic';
const selectedOpacityKey=()=>({basic:'discOpacityBasic',cd:'discOpacityCd',vinyl:'discOpacityVinyl','vinyl-cover':'discOpacityVinylCover','cd-cover':draft.cdCoverSide==='up'?'discOpacityCdCoverUp':'discOpacityCdCoverDown','album-cover':'discOpacityAlbumCover'})[draft.discAppearance||'basic'];
const selectedHoleKey=()=>holeKey(draft.discAppearance||'basic');
const appearanceSuffix=()=>({basic:'Basic',cd:'Cd',vinyl:'Vinyl','vinyl-cover':'VinylCover','cd-cover':'CdCover','album-cover':'AlbumCover'})[draft.discAppearance||'vinyl-cover'];
const selectedSizeKey=()=>`discSize${appearanceSuffix()}`;
const presetKeys=new Set(['transitionStyle','transitionDuration','transitionDelay','sleeveTransitionStyle','sleeveAlignment','sleeveDuration','sleeveDelayIn','sleeveDelayOut']);
const selectedFieldKey=key=>presetKeys.has(key)&&(!key.startsWith('sleeve')||isCover(draft.discAppearance))?key+appearanceSuffix():key;
const selectedValue=key=>draft[selectedFieldKey(key)]??draft[key];
const selectedHoleColorKey=()=>selectedHoleKey().replace('centerHole','centerHoleColor');

function format(value,suffix=''){
  const text=typeof value==='number'?String(Number(value.toFixed(2))):String(value);
  return suffix?text+' '+suffix:text;
}

function renderNav(){
  nav.replaceChildren();
  const selectPage=(index)=>{
    activeSection=index;
    renderNav();
    renderSection();
  };
  const makePage=(section,index,subpage=false)=>{
    const button=document.createElement('button');
    button.type='button';
    button.className='nav-button'+(subpage?' nav-subbutton':'');
    button.setAttribute('aria-current',index===activeSection?'page':'false');
    button.textContent=subpage?motionLabel(section.title):section.title;
    button.addEventListener('click',()=>selectPage(index));
    return button;
  };
  sections.forEach((section,index)=>{
    if(!motionPages.has(section.title)){
      nav.append(makePage(section,index));
      return;
    }
    if(section.title!=='Sleeve motion')return;
    const group=document.createElement('div');
    group.className='nav-group';
    const toggle=document.createElement('button');
    toggle.type='button';
    toggle.className='nav-button nav-group-toggle';
    toggle.setAttribute('aria-expanded',String(motionExpanded));
    toggle.setAttribute('aria-controls','motion-subnav');
    toggle.textContent='Motion';
    toggle.addEventListener('click',()=>{
      motionExpanded=!motionExpanded;
      renderNav();
    });
    group.append(toggle);
    const subnav=document.createElement('div');
    subnav.id='motion-subnav';
    subnav.className='nav-subnav';
    subnav.hidden=!motionExpanded;
    sections.forEach((page,pageIndex)=>{
      if(motionPages.has(page.title)&&motionVisible(page.title))subnav.append(makePage(page,pageIndex,true));
    });
    group.append(subnav);
    nav.append(group);
  });
}

function renderSection(){
  if(!motionVisible(sections[activeSection].title)){
    activeSection=sections.findIndex(section=>section.title==='Disc motion');
    renderNav();
  }
  const hidden=key=>
    (sections[activeSection].title==='Panel'&&key!=='panelEnabled'&&draft.panelEnabled===false)||
    (sections[activeSection].title==='Panel motion'&&draft.panelEnabled===false)||(key==='cdCoverSide'&&draft.discAppearance!=='cd-cover')||
    (key==='cdRainbow'&&(draft.discAppearance!=='cd-cover'||draft.cdCoverSide!=='down'))||
    (['centerHole','centerHoleColor','discRotationSeconds'].includes(key)&&draft.discAppearance==='album-cover')||
    (['vinylGrooveDensity','vinylLabelSize'].includes(key)&&!['vinyl','vinyl-cover'].includes(draft.discAppearance))||
    (sections[activeSection].title==='Sleeve motion'&&!isCover(draft.discAppearance))||
    
    (key==='sleeveDuration'&&selectedValue('sleeveTransitionStyle')==='off')||
    (['sleeveDelayIn','sleeveDelayOut'].includes(key)&&selectedValue('sleeveTransitionStyle')==='off')||
    (key==='sleeveAlignment'&&!isCover(draft.discAppearance))||
    (key==='timeLayout'&&draft.timeDisplay==='off')||
    (key==='transitionDuration'&&selectedValue('transitionStyle')==='off')||
    (key==='transitionDelay'&&selectedValue('transitionStyle')==='off')||
    (key==='backdropTransitionDelay'&&draft.backdropTransitionStyle==='off')||
    (['panelDelayIn','panelDelayOut'].includes(key)&&draft.panelTransitionStyle==='off')||
    (key==='backdropTransitionDuration'&&draft.backdropTransitionStyle==='off')||
    (key==='panelTransitionDuration'&&draft.panelTransitionStyle==='off');
  const section=sections[activeSection];
  document.querySelector('#panel-kicker').textContent='';
  document.querySelector('#panel-title').textContent=motionPages.has(section.title)?motionLabel(section.title):section.title;
  document.querySelector('#panel-description').textContent='';
  controls.replaceChildren();

  for(const [key,label,type,min,max,step,suffix] of section.fields){
    const line=document.createElement('div');
    line.className='field';
    if(hidden(key))continue;
    const caption=document.createElement('label');
    caption.className='field-label';
    caption.htmlFor=key;
    caption.textContent=key==='discSize'&&['vinyl-cover','cd-cover','album-cover'].includes(draft.discAppearance)?'Cover size':label;
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
      for(const choice of min){const option=document.createElement('option');option.value=String(choice);option.textContent=({song:'Song change',album:'Album change',cd:'CD','vinyl-cover':'Vinyl & Cover','cd-cover':'CD & Cover','album-cover':'Album cover'}[choice]||String(choice).replace(/^./,c=>c.toUpperCase()));select.append(option)}
      select.value=String((key==='centerHoleColor'?draft[selectedHoleColorKey()]:selectedValue(key))??min[0]);
      line.append(select);
      const output=document.createElement('output');output.htmlFor=key;
      const refresh=()=>{const value=typeof min[0]==='number'?Number(select.value):select.value;draft[key==='centerHoleColor'?selectedHoleColorKey():selectedFieldKey(key)]=value;output.textContent=String(value);};
      select.addEventListener('change',()=>{refresh();if(['discAppearance','cdCoverSide','timeDisplay','transitionStyle','sleeveTransitionStyle','backdropTransitionStyle','panelTransitionStyle'].includes(key)){if(key==='discAppearance')renderNav();renderSection();}status.textContent='Unsaved changes';});
      // Rendering a select must not change the configuration. Only user input does.
      output.textContent=select.value;
      controls.append(line);continue;
    }else if(type==='range'){
      input.type='range';
      input.min=min;
      input.max=max;
      input.step=step;
      input.value=(key==='centerHole'?draft[selectedHoleKey()]:key==='discSize'?draft[selectedSizeKey()]:key==='discOpacity'?draft[selectedOpacityKey()]:selectedValue(key))??min;
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
      draft[key==='centerHole'?selectedHoleKey():key==='discSize'?selectedSizeKey():key==='discOpacity'?selectedOpacityKey():selectedFieldKey(key)]=value;
      output.textContent=type==='toggle'?(value?'On':'Off'):type==='text'?'Local':format(value,suffix);
    };
    input.addEventListener('input',()=>{
      refresh();
      if(key==='panelEnabled')renderSection();
      status.textContent='Unsaved changes';
    });
    if(!['centerHole','discOpacity',...presetKeys].includes(key))refresh();else output.textContent=format(Number(input.value),suffix);
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
  delete draft['centerHoleColorAlbum-cover'];
  delete draft['centerHoleAlbum-cover'];
  for(const key of Object.keys(draft))if(/^sleeve(?:TransitionStyle|Alignment|Duration|DelayIn|DelayOut)AlbumCover$/.test(key))delete draft[key];
  draft.changeDiscOn=draft.changeDiscOn==='song'?'song':'album';
  draft.discAppearance ||= 'vinyl-cover';
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
    delete draft['centerHoleColorAlbum-cover'];
    delete draft['centerHoleAlbum-cover'];
  for(const key of Object.keys(draft))if(/^sleeve(?:TransitionStyle|Alignment|Duration|DelayIn|DelayOut)AlbumCover$/.test(key))delete draft[key];
    draft.changeDiscOn=draft.changeDiscOn==='song'?'song':'album';
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
