import {artistFor,coverFor,normalize,positionMs,durationMs,playing as isPlaying} from './soloist.js';
import {DiscDirector} from './motion.js';
import {Spectrum} from './spectrum.js';

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
function connect(url,onData,onOpen){
  let retry=500;
  function open(){
    let socket;try{socket=new WebSocket(url)}catch{setTimeout(open,retry);return}
    socket.onopen=()=>{retry=500;onOpen?.(socket)};
    socket.onmessage=e=>{try{onData(JSON.parse(e.data))}catch(err){console.warn('WebSocket event',err)}};
    socket.onclose=()=>{setTimeout(open,retry);retry=Math.min(retry*1.7,12000)};
    socket.onerror=()=>socket.close();
  }
  open();
}
connect(config.soloistWebSocket||'ws://127.0.0.1:9090',handleSoloist,s=>s.send(JSON.stringify({type:'command',command:'get_state'})));
if(config.visualizer!==false){
  let retry=500;
  function connectAudio(){
    const socket=new WebSocket(config.audioWebSocket||'ws://127.0.0.1:9091');
    socket.onopen=()=>{retry=500};
    socket.onmessage=event=>{try{spectrum.receive(JSON.parse(event.data))}catch(error){console.warn('Invalid spectrum frame',error)}};
    socket.onclose=()=>{setTimeout(connectAudio,retry);retry=Math.min(12000,retry*1.7)};
    socket.onerror=()=>socket.close();
  }
  connectAudio();
}
function progressFrame(now){
  const elapsed=playing?now-positionTime:0;
  const ratio=duration?Math.max(0,Math.min(1,(position+elapsed)/duration)):0;
  $('progress-fill').style.width=`${ratio*100}%`;
  $('progress').setAttribute('aria-valuenow',String(Math.round(ratio*100)));
  requestAnimationFrame(progressFrame);
}
requestAnimationFrame(progressFrame);
