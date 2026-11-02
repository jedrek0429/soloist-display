import {artistFor,coverFor,normalize,positionMs,durationMs,playing as isPlaying} from './soloist.js';
import {DiscDirector} from './motion.js';
import {Spectrum} from './spectrum.js';

const updateVersion=new URLSearchParams(location.search).get('update');
if(/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(updateVersion||'')&&localStorage.getItem('soloistDisplayUpdateNotice')!==updateVersion){
  localStorage.setItem('soloistDisplayUpdateNotice',updateVersion);
  setTimeout(()=>alert(`Soloist Display ${updateVersion} is available. Run “soloist-display update” to install it.`),500);
}

const config=await fetch('./config.json').then(r=>r.json()).catch(()=>({}));
// Settings saved in the other launcher are picked up without restarting this display.
let configSnapshot=JSON.stringify(config);
setInterval(async()=>{
  try{
    const updated=await fetch('./config.json',{cache:'no-store'}).then(r=>r.json());
    const snapshot=JSON.stringify(updated);
    if(snapshot!==configSnapshot)location.reload();
  }catch{}
},2000);
const $=id=>document.getElementById(id);
const bounded=(value,fallback,min,max)=>Number.isFinite(Number(value))?Math.max(min,Math.min(max,Number(value))):fallback;
const settings={
  discSize:bounded(config.discSize,116,50,200),
  discRotationSeconds:bounded(config.discRotationSeconds,12,1,120),
  transitionDuration:bounded(config.transitionDuration,2000,100,10000),
  backdropTransitionStyle:config.backdropTransitionStyle||'fade',
  backdropTransitionDuration:bounded(config.backdropTransitionDuration,2000,100,10000),
  backgroundDim:bounded(config.backgroundDim,.52,0,1),
  backgroundBlur:bounded(config.backgroundBlur,50,0,150),
  metadataOpacity:bounded(config.metadataOpacity,.58,0,1),
  metadataScale:bounded(config.metadataScale,1,.5,2),
  panelFontSize:bounded(config.panelFontSize,1,.6,1.6),
  panelTransitionStyle:config.panelTransitionStyle||'slide',
  panelTransitionDuration:bounded(config.panelTransitionDuration,2050,100,10000),
  reducedMotion:config.reducedMotion===true,
  transitionStyle:config.transitionStyle||'fly'
};
const root=document.documentElement;
root.dataset.progressBar=config.progressBar===false?'off':'on';
root.dataset.timeDisplay=['off','remaining','total'].includes(config.timeDisplay)?config.timeDisplay:'off';
root.dataset.timeLayout=config.timeLayout==='together'?'together':'split';
root.dataset.discAppearance=['basic','cd','vinyl'].includes(config.discAppearance)?config.discAppearance:'basic';
root.dataset.panelPosition=['bottom-right','bottom-left','top-right','top-left'].includes(config.panelPosition)?config.panelPosition:'bottom-right';
const holeKey='centerHole'+root.dataset.discAppearance[0].toUpperCase()+root.dataset.discAppearance.slice(1);
const holeSize=bounded(config[holeKey],root.dataset.discAppearance==='cd'?12:root.dataset.discAppearance==='vinyl'?.6:1,0,20);
const colorKey=holeKey.replace('centerHole','centerHoleColor');
const holeColor=['white','black','transparent'].includes(config[colorKey])?config[colorKey]:'transparent';
root.dataset.holeColor=holeColor;
root.style.setProperty('--center-hole-radius',holeSize/2+'%');
root.style.setProperty('--center-hole-diameter',holeSize+'%');
if(config.burnInProtection===true)root.classList.add('burn-in-protection');
for(const [key,value] of Object.entries({
  '--disc-size':settings.discSize+'dvh',
  '--background-dim':settings.backgroundDim,
  '--background-blur':settings.backgroundBlur+'px',
  '--metadata-opacity':settings.metadataOpacity,
  '--metadata-font-size':settings.panelFontSize,
  '--metadata-scale':settings.metadataScale
}))root.style.setProperty(key,value);
const director=new DiscDirector({discs:[$('disc-a'),$('disc-b')],backdrops:[$('backdrop-a'),$('backdrop-b')],info:$('info'),settings});
const spectrum=new Spectrum($('visualizer'), config.visualizerSensitivity ?? 1, config.visualizerSmoothing ?? 0.7, {autoNormalize:config.visualizerAutoNormalize===true,peakHold:config.visualizerPeakHold===true});
let currentUri=null,position=0,positionTime=performance.now(),duration=0,playing=false;
let lastTrack=null;
function updateItem(item){
  if(!item)return;
  lastTrack=item;
  const uri=item.uri||item.id||[item?.decorations?.identity?.name,coverFor(item)].join('|');
  duration=durationMs(item)||duration;
  if(uri===currentUri)return;
  currentUri=uri;
  const name=item?.decorations?.identity?.name||item.name||'Not playing';
  const artist=artistFor(item),cover=coverFor(item);
  director.change(cover,()=>{$('track').textContent=name;$('artist').textContent=artist});
}
function handleSoloist(event){
  const {item,position:pos,status}=normalize(event);
  if(item)updateItem(item);
  if(status!==null){playing=isPlaying(status);director.setPlaying(playing)}
  if(pos!==null){const ms=positionMs(pos);if(Number.isFinite(ms)){position=ms;positionTime=performance.now()}}
  if(event.type==='position_sync'||event.type==='position_changed'){const ms=positionMs(event.position??event);if(Number.isFinite(ms)){position=ms;positionTime=performance.now()}}
  if(event.type==='playback_changed'&&event.status!==undefined){playing=isPlaying(event.status);director.setPlaying(playing)}
}
function connect(url,onData,onOpen,onDisconnect){
  let retry=500;
  function open(){
    let socket;try{socket=new WebSocket(url)}catch{onDisconnect?.();setTimeout(open,retry);return}
    socket.onopen=()=>{retry=500;onOpen?.(socket)};
    socket.onmessage=e=>{try{onData(JSON.parse(e.data))}catch(err){console.warn('WebSocket event',err)}};
    socket.onclose=()=>{onDisconnect?.();setTimeout(open,retry);retry=Math.min(retry*1.7,12000)};
    socket.onerror=()=>socket.close();
  }
  open();
}
connect(config.soloistWebSocket||'ws://127.0.0.1:9090',handleSoloist,s=>{
  $('track').textContent=currentUri?$('track').textContent:'Waiting for playback';
  s.send(JSON.stringify({type:'command',command:'get_state'}));
},()=>{
  playing=false;director.setPlaying(false);currentUri=null;
  $('track').textContent='Soloist disconnected';
  $('artist').textContent='Reconnecting automatically…';
});
if(config.visualizer!==false){
  let retry=500;
  function connectAudio(){
    let socket;
    try{socket=new WebSocket(config.audioWebSocket||'ws://127.0.0.1:9091')}catch(error){console.warn('Audio connection failed',error);setTimeout(connectAudio,retry);retry=Math.min(12000,retry*1.7);return;}
    socket.onopen=()=>{retry=500};
    socket.onmessage=event=>{try{spectrum.receive(JSON.parse(event.data))}catch(error){console.warn('Invalid spectrum frame',error)}};
    socket.onclose=()=>{setTimeout(connectAudio,retry);retry=Math.min(12000,retry*1.7)};
    socket.onerror=()=>socket.close();
  }
  connectAudio();
}
const formatTime=ms=>{const seconds=Math.floor(Math.max(0,ms)/1000);const minutes=Math.floor(seconds/60);return minutes+':'+String(seconds%60).padStart(2,'0');};
function progressFrame(now){
  const elapsed=playing?now-positionTime:0;
  const ratio=duration?Math.max(0,Math.min(1,(position+elapsed)/duration)):0;
  $('progress-fill').style.width=`${ratio*100}%`;
  if(root.dataset.timeDisplay!=='off'){
    const elapsedMs=Math.max(0,Math.min(duration||0,position+elapsed));
    $('time-elapsed').textContent=formatTime(elapsedMs);
    $('time-end').textContent=root.dataset.timeDisplay==='remaining'?'−'+formatTime(Math.max(0,duration-elapsedMs)):formatTime(duration);
  }
  $('progress').setAttribute('aria-valuenow',String(Math.round(ratio*100)));
  requestAnimationFrame(progressFrame);
}
requestAnimationFrame(progressFrame);
