import {artistFor,coverFor,normalize,positionMs,durationMs,playing as isPlaying} from './soloist.js';
import {DiscDirector} from './motion.js';
import {Spectrum} from './spectrum.js';

const config=await fetch('./config.json').then(r=>r.json()).catch(()=>({}));
const $=id=>document.getElementById(id);
const director=new DiscDirector({discs:[$('disc-a'),$('disc-b')],backdrops:[$('backdrop-a'),$('backdrop-b')],info:$('info')});
const spectrum=new Spectrum($('visualizer'));
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
    socket.binaryType='arraybuffer';
    socket.onopen=()=>{retry=500};
    socket.onmessage=event=>spectrum.receive(event.data);
    socket.onclose=()=>{setTimeout(connectAudio,retry);retry=Math.min(12000,retry*1.7)};
    socket.onerror=()=>socket.close();
  }
  connectAudio();
  // Firefox may initially suspend Web Audio until the first user interaction.
  const unlock=()=>spectrum.context?.resume().catch(()=>{});
  window.addEventListener('pointerdown',unlock);
  window.addEventListener('keydown',unlock);
}
function progressFrame(now){
  const elapsed=playing?now-positionTime:0;
  const ratio=duration?Math.max(0,Math.min(1,(position+elapsed)/duration)):0;
  $('progress-fill').style.width=`${ratio*100}%`;
  $('progress').setAttribute('aria-valuenow',String(Math.round(ratio*100)));
  requestAnimationFrame(progressFrame);
}
requestAnimationFrame(progressFrame);
