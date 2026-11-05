import {artistFor,albumKeyFor,coverFor,normalize,positionMs,durationMs,playing as isPlaying,isEmptyPlayback,pauseDelayMs} from './soloist.js';
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
const appearances=['basic','cd','vinyl','vinyl-cover','cd-cover','album-cover'];
const appearance=appearances.includes(config.discAppearance)?config.discAppearance:'vinyl-cover';
const suffix={basic:'Basic',cd:'Cd',vinyl:'Vinyl','vinyl-cover':'VinylCover','cd-cover':'CdCover','album-cover':'AlbumCover'}[appearance];
const motionValue=key=>config[key+suffix]??config[key];
const settings={
  discSize:bounded(config['discSize'+suffix],bounded(config.discSize,116,50,200),50,200),
  changeDiscOn:config.changeDiscOn==='song'?'song':'album',
  discRotationSeconds:bounded(config.discRotationSeconds,12,1,120),
  transitionDuration:bounded(motionValue('transitionDuration'),2000,100,10000),
  backdropTransitionStyle:config.backdropTransitionStyle||'fade',
  backdropTransitionDuration:bounded(config.backdropTransitionDuration,2000,100,10000),
  backgroundDim:bounded(config.backgroundDim,.52,0,1),
  backgroundBlur:bounded(config.backgroundBlur,50,0,150),
  metadataOpacity:bounded(config.metadataOpacity,.58,0,1),
  metadataScale:bounded(config.metadataScale,1,.5,2),
  panelFontSize:bounded(config.panelFontSize,1,.6,1.6),
  panelEnabled:config.panelEnabled!==false,
  panelTransitionStyle:config.panelTransitionStyle||'slide',
  panelTransitionDuration:bounded(config.panelTransitionDuration,2050,100,10000),
  panelHideMs:pauseDelayMs(config.panelHideDelaySeconds??-1),
  reducedMotion:config.reducedMotion===true,
  transitionStyle:motionValue('transitionStyle')||'fly',
  sleeveTransitionStyle:motionValue('sleeveTransitionStyle')||'in-out',
  sleeveDuration:bounded(motionValue('sleeveDuration'),1000,100,10000),
  pauseRetractMs:pauseDelayMs(motionValue('pauseRetractDelaySeconds')??-1),
  sleeveAlignment:motionValue('sleeveAlignment')==='left'?'left':'center',
  sleeveDelayIn:bounded(motionValue('sleeveDelayIn'),0,0,10000),
  sleeveDelayOut:bounded(motionValue('sleeveDelayOut'),950,0,10000),
  transitionDelay:bounded(motionValue('transitionDelay'),1000,0,10000),
  backdropTransitionDelay:bounded(config.backdropTransitionDelay,1000,0,10000),
  panelDelayIn:bounded(config.panelDelayIn ?? config.panelTransitionDelay,0,0,10000),
  panelDelayOut:bounded(config.panelDelayOut,0,0,10000)
};
const root=document.documentElement;
root.dataset.reducedMotion=settings.reducedMotion?'true':'false';
root.dataset.idleAppearance=['digital-clock','vinyl-clock','vinyl','text','black'].includes(config.idleAppearance)?config.idleAppearance:'digital-clock';
root.dataset.idleShowDate=config.idleShowDate===false?'false':'true';
root.dataset.idleTransition=config.idleTransitionStyle==='off'?'off':'fade';
$('standby-title').textContent=typeof config.idleText==='string'?config.idleText.slice(0,80):'';
function updateStandbyClock(){
  const now=new Date();
  $('standby-clock').textContent=now.toLocaleTimeString(undefined,{hour:'2-digit',minute:'2-digit',hourCycle:'h23'});
  $('standby-date').textContent=now.toLocaleDateString(undefined,{weekday:'long',month:'long',day:'numeric'});
  const hour=(now.getHours()%12)+now.getMinutes()/60+now.getSeconds()/3600;
  const minute=now.getMinutes()+now.getSeconds()/60;
  $('standby-record').style.setProperty('--standby-hour-angle',hour*30+'deg');
  $('standby-record').style.setProperty('--standby-minute-angle',minute*6+'deg');
}
updateStandbyClock();
if(['digital-clock','vinyl-clock'].includes(root.dataset.idleAppearance))setInterval(updateStandbyClock,1000);
root.dataset.progressBar=config.progressBar===false?'off':'on';
root.dataset.timeDisplay=['off','remaining','total'].includes(config.timeDisplay)?config.timeDisplay:'off';
root.dataset.timeLayout=config.timeLayout==='together'?'together':'split';
root.dataset.discAppearance=['basic','cd','vinyl','vinyl-cover','cd-cover','album-cover'].includes(config.discAppearance)?config.discAppearance:'vinyl-cover';
root.dataset.cdCoverSide=config.cdCoverSide==='up'?'up':'down';
root.style.setProperty('--cd-rainbow',bounded(config.cdRainbow,.5,0,1));
root.style.setProperty('--vinyl-label-size',bounded(config.vinylLabelSize,34,10,70)+'%');
root.style.setProperty('--vinyl-groove-spacing', (11-Math.max(1,Math.min(10,Number(config.vinylGrooveDensity)||5)))*1.1+'px');
root.dataset.panelEnabled=settings.panelEnabled?'on':'off';
root.dataset.panelPosition=['bottom-right','bottom-left','top-right','top-left'].includes(config.panelPosition)?config.panelPosition:'bottom-right';
const opacityKey=({basic:'discOpacityBasic',cd:'discOpacityCd',vinyl:'discOpacityVinyl','vinyl-cover':'discOpacityVinylCover','cd-cover':root.dataset.cdCoverSide==='up'?'discOpacityCdCoverUp':'discOpacityCdCoverDown','album-cover':'discOpacityAlbumCover'})[root.dataset.discAppearance];
root.style.setProperty('--disc-opacity',bounded(config[opacityKey],opacityKey==='discOpacityCdCoverDown'?.7:1,0,1));
const holeKey='centerHole'+(['vinyl','vinyl-cover'].includes(root.dataset.discAppearance)?'Vinyl':['cd','cd-cover'].includes(root.dataset.discAppearance)?'Cd':'Basic');
const holeSize=bounded(config[holeKey],['cd','cd-cover'].includes(root.dataset.discAppearance)?14:['vinyl','vinyl-cover'].includes(root.dataset.discAppearance)?.6:1,0,20);
const colorKey=holeKey.replace('centerHole','centerHoleColor');
const holeColor=['white','black','transparent'].includes(config[colorKey])?config[colorKey]:'transparent';
root.dataset.holeColor=holeColor;
root.style.setProperty('--center-hole-radius',holeSize/2+'%');
root.style.setProperty('--center-hole-diameter',holeSize+'%');
// closest-side radial stops are relative to disc radius: X% stop = X% disc diameter.
root.style.setProperty('--cd-hole-stop',holeSize+'%');
if(config.burnInProtection===true)root.classList.add('burn-in-protection');
for(const [key,value] of Object.entries({
  '--disc-size':settings.discSize+'dvh',
  '--background-dim':settings.backgroundDim,
  '--background-blur':settings.backgroundBlur+'px',
  '--metadata-opacity':settings.metadataOpacity,
  '--metadata-font-size':settings.panelFontSize,
  '--metadata-scale':settings.metadataScale,
  '--idle-fade-duration':bounded(config.idleFadeDuration,300,0,10000)+'ms'
}))root.style.setProperty(key,value);
const director=new DiscDirector({discs:[$('disc-a'),$('disc-b')],backdrops:[$('backdrop-a'),$('backdrop-b')],info:$('info'),settings});
const spectrum=new Spectrum($('visualizer'), config.visualizerSensitivity ?? 1, config.visualizerSmoothing ?? 0.7, {autoNormalize:config.visualizerAutoNormalize===true,peakHold:config.visualizerPeakHold===true});
let connectionNoticeTimer=null,connectedNoticePending=false;
function showConnectedNotice(){
  clearTimeout(connectionNoticeTimer);
  $('standby-connection').textContent='Connected!';
  connectionNoticeTimer=setTimeout(()=>{
    connectionNoticeTimer=null;
    $('standby-connection').textContent='';
  },2000);
}
function setDisplayState(state){
  root.dataset.displayState=state;
  if(state!=='connecting')root.dataset.initializing='false';
  if(state==='ready'&&connectedNoticePending){
    connectedNoticePending=false;
    showConnectedNotice();
    return;
  }
  if(state!=='ready')connectedNoticePending=false;
  const notice=$('standby-connection');
  const labels={connecting:'Connecting...',offline:'Disconnected',reconnecting:'Reconnecting...'};
  if(labels[state]){
    clearTimeout(connectionNoticeTimer);
    connectionNoticeTimer=null;
    notice.textContent=labels[state];
  }else if(connectionNoticeTimer===null||state!=='ready'){
    clearTimeout(connectionNoticeTimer);
    connectionNoticeTimer=null;
    notice.textContent='';
  }
}
let currentUri=null,position=0,positionTime=performance.now(),duration=0,playing=false;
let pauseTimer=null;
function cancelPauseTimer(){
  if(pauseTimer!==null)clearTimeout(pauseTimer);
  pauseTimer=null;
}
function showPausedAfterDelay(){
  if(pauseTimer!==null||!currentUri)return;
  const wait=pauseDelayMs(config.idlePauseDelaySeconds??300);
  if(wait===null)return;
  if(wait===0){setDisplayState('paused');return;}
  pauseTimer=setTimeout(()=>{
    pauseTimer=null;
    if(!playing&&currentUri)setDisplayState('paused');
  },wait);
}
function clearTrack(){
  cancelPauseTimer();
  root.dataset.metadataReady='false';
  $('track').textContent='';
  $('artist').textContent='';
  if(currentUri!==null||director.started){
    director.reset();
    currentUri=null;
    currentAlbumKey=null;
    lastTrack=null;
    position=0;
    duration=0;
    positionTime=performance.now();
  }
  playing=false;
  director.setPlaying(false);
}

let lastTrack=null;
let currentAlbumKey=null;
function updateItem(item){
  if(!item)return;
  const name=item?.decorations?.identity?.name||item.name||'';
  const cover=coverFor(item);
  // A partial state without any track metadata is not an album to animate.
  if(!name.trim()&&!cover)return;
  lastTrack=item;
  const uri=item.uri||item.id||[name,cover].join('|');
  duration=durationMs(item)||duration;
  if(uri===currentUri)return;
  cancelPauseTimer();
  const fromIdle=root.dataset.displayState!=='playing';
  // Idle may retain a paused album internally. It is not a visible outgoing disc.
  if(fromIdle&&director.started)director.reset();
  position=0;positionTime=performance.now();
  currentUri=uri;
  const albumKey=albumKeyFor(item);
  const panelOnly=settings.changeDiscOn==='album'&&albumKey!==null&&albumKey===currentAlbumKey&&director.started;
  currentAlbumKey=albumKey;
  const artist=artistFor(item);
  const show=()=>{
    $('track').textContent=name;
    $('artist').textContent=artist;
    root.dataset.metadataReady=name.trim()?'true':'false';
  };
  if(panelOnly){
    setDisplayState('playing');
    director.changePanel(show);
  }else{
    if(!fromIdle)setDisplayState('playing');
    void director.change(cover,()=>{
      show();
      if(fromIdle)setDisplayState('playing');
    });
  }
}
function syncPosition(ms){
  const now=performance.now();
  const estimated=position+(playing?now-positionTime:0);
  // Ignore jitter; seek or genuine drift should still correct the clock.
  if(!positionTime||Math.abs(ms-estimated)>750){position=ms;positionTime=now}
}
function handleSoloist(event){
  const {item,position:pos,status}=normalize(event);
  if(isEmptyPlayback(event)){
    clearTrack();
    setDisplayState('ready');
    return;
  }
  if(item)updateItem(item);
  if(status!==null){
    playing=isPlaying(status);
    director.setPlaying(playing,{instantResume:playing&&root.dataset.displayState==='paused'});
    if(playing){
      cancelPauseTimer();
      if(currentUri&&director.started)setDisplayState('playing');
    }else if(currentUri){
      showPausedAfterDelay();
    }
  }
  if(pos!==null){const ms=positionMs(pos);if(Number.isFinite(ms))syncPosition(ms)}
  if(event.type==='position_sync'||event.type==='position_changed'){
    const ms=positionMs(event.position??event);
    if(Number.isFinite(ms))syncPosition(ms);
  }
}
function connect(url,onData,onOpen,onDisconnect,onConnecting){
  let retry=500,connected=false;
  function lost(){
    onDisconnect?.(connected?'reconnecting':'offline');
    setTimeout(open,retry);
    retry=Math.min(retry*1.7,12000);
  }
  function open(){
    onConnecting?.();
    let socket;
    try{socket=new WebSocket(url)}catch{lost();return}
    socket.onopen=()=>{connected=true;retry=500;onOpen?.(socket)};
    socket.onmessage=e=>{try{onData(JSON.parse(e.data))}catch(err){console.warn('WebSocket event',err)}};
    socket.onclose=lost;
    socket.onerror=()=>socket.close();
  }
  open();
}
connect(config.soloistWebSocket||'ws://127.0.0.1:9090',handleSoloist,s=>{
  connectedNoticePending=!currentUri;
  if(!currentUri){
    if(root.dataset.initializing==='false')setDisplayState('ready');
    else setTimeout(()=>{
      // An empty get_state response is not guaranteed. Do not stay blank forever.
      if(!currentUri&&s.readyState===WebSocket.OPEN&&root.dataset.displayState==='connecting')setDisplayState('ready');
    },600);
  }
  s.send(JSON.stringify({type:'command',command:'get_state'}));
},state=>{
  clearTrack();
  setDisplayState(state);
},()=>{
  if(!currentUri)setDisplayState('connecting');
});
if(config.visualizer!==false&&settings.panelEnabled){
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
