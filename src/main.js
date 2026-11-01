const config=await fetch('./config.json').then(r=>r.json()).catch(()=>({}));
const $=id=>document.getElementById(id);
const stage=$('stage'),disc=$('disc'),rotor=$('disc-rotor'),art=$('disc-image'),title=$('track'),artist=$('artist'),fill=$('progress-fill'),progress=$('progress');
const canvas=$('visualizer'),ctx=canvas.getContext('2d');
const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let trackId=null,transitionId=0,playing=false,angularVelocity=0,angle=0,lastFrame=performance.now();
let duration=0,positionMs=0,positionAt=performance.now(),bars=Array(48).fill(0),smoothed=Array(48).fill(0),audioAt=0;
let latestItem=null;
const pick=(...v)=>v.find(x=>x!==undefined&&x!==null&&x!=='');
const nameOf=v=>typeof v==='string'?v:pick(v?.decorations?.identity?.name,v?.name,v?.title,'');
function trackName(item){return nameOf(item)||'Not playing'}
function artistName(item){const parent=pick(item?.parent?.entity,item?.parent,item?.album?.artists?.[0]);const creators=item?.decorations?.creators||item?.artists||[];return pick(nameOf(parent),creators.map(nameOf).filter(Boolean).join(', '),item?.artist,'Unknown artist')}
function artworkUrl(item){const cover=pick(item?.decorations?.visual_identity?.cover,item?.parent?.entity?.decorations?.visual_identity?.cover,item?.images,[]);if(typeof cover==='string')return cover;if(!Array.isArray(cover))return cover?.url||'';return cover.find(x=>x?.size==='large')?.url||cover.find(x=>x?.width>=500)?.url||cover.at(-1)?.url||cover[0]?.url||''}
function identity(item){return pick(item?.uri,item?.id,item?.decorations?.identity?.uri,[trackName(item),artistName(item),artworkUrl(item)].join('|'))}
function isPlaying(status){if(typeof status==='boolean')return status;if(typeof status==='object')return isPlaying(pick(status?.state,status?.status,status?.playback_status));return ['playing','play','true'].includes(String(status).toLowerCase())}
function durationOf(item){return Number(pick(item?.decorations?.playback?.duration_ms,item?.duration_ms,item?.duration,0))||0}
function syncPosition(pos){if(pos==null)return;const value=typeof pos==='number'?pos:pick(pos.position_ms,pos.position,pos.ms);if(Number.isFinite(Number(value))){positionMs=Math.max(0,Number(value));positionAt=performance.now()}}
function paletteFrom(image){try{const c=document.createElement('canvas');c.width=c.height=40;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0,40,40);const d=x.getImageData(0,0,40,40).data;const bins=new Map();for(let i=0;i<d.length;i+=16){if(d[i+3]<200)continue;const r=d[i],g=d[i+1],b=d[i+2];const max=Math.max(r,g,b),min=Math.min(r,g,b);if(max<25||min>235)continue;const key=[r>>5,g>>5,b>>5].join(',');const old=bins.get(key)||{r:0,g:0,b:0,n:0};old.r+=r;old.g+=g;old.b+=b;old.n++;bins.set(key,old)}const winner=[...bins.values()].sort((a,b)=>b.n-a.n)[0];if(!winner)return null;return [winner.r/winner.n,winner.g/winner.n,winner.b/winner.n]}catch{return null}}
function applyPalette(rgb){if(!rgb)return;const [r,g,b]=rgb.map(Math.round);const root=document.documentElement;root.style.setProperty('--bg',`color-mix(in oklch,rgb(${r} ${g} ${b}) 16%, #454541)`);root.style.setProperty('--accent',`color-mix(in oklch,rgb(${r} ${g} ${b}) 64%, #f3f0e9)`);root.style.setProperty('--muted','#dedbd4')}
function preload(url){return new Promise(resolve=>{if(!url)return resolve(null);const image=new Image();image.crossOrigin='anonymous';image.onload=()=>resolve(image);image.onerror=()=>resolve(null);image.src=url})}
function paintArtwork(url){art.style.backgroundImage=url?'url('+JSON.stringify(url)+')':'none'}
async function showTrack(item){
  if(!item)return;
  const id=identity(item);if(id===trackId)return;
  trackId=id;const generation=++transitionId;const url=artworkUrl(item),name=trackName(item),by=artistName(item);
  const image=await preload(url);if(generation!==transitionId)return;
  const colors=image?paletteFrom(image):null;
  if(reduced){paintArtwork(url);title.textContent=name;artist.textContent=by;applyPalette(colors);stage.className='stage';return}
  // One disc travels out of the composition; the replacement arrives oversized.
  const previous=disc.animate([{transform:'translate(-50%,-50%) scale(1)',opacity:1},{transform:'translate(-150%,-50%) scale(.92)',opacity:1}],{duration:440,easing:'cubic-bezier(.5,0,.75,.25)',fill:'forwards'});
  try{await previous.finished}catch{}
  if(generation!==transitionId){previous.cancel();return}
  previous.cancel();
  stage.className='stage showcase';
  paintArtwork(url);applyPalette(colors);
  title.textContent=name;artist.textContent=by;
  // Enter from the opposite edge, already oversized, then hold the artwork as a hero shot.
  const entering=disc.animate([{transform:'translate(35%,-50%) scale(1.34)'},{transform:'translate(-50%,-50%) scale(1.34)'}],{duration:620,easing:'cubic-bezier(.17,.75,.2,1)'});
  try{await entering.finished}catch{}
  if(generation!==transitionId)return;
  await sleep(1000);if(generation!==transitionId)return;
  stage.className='stage transitioning';
  requestAnimationFrame(()=>{stage.classList.remove('showcase')});
  await sleep(1120);if(generation!==transitionId)return;
  stage.className='stage';
}
function updatePlayback(event){const item=pick(event.item,event.playback?.item,event.state?.item);if(item){latestItem=item;duration=durationOf(item)||duration;void showTrack(item)}const status=pick(event.status,event.playback?.status,event.state?.status,event.playback_status);if(status!==undefined)playing=isPlaying(status);syncPosition(pick(event.position,event.playback?.position,event.state?.position))}
function onSoloist(event){const type=event?.type;if(type==='playback_state'||type==='state'||type==='state_snapshot')updatePlayback(event);else if(type==='track_changed')updatePlayback(event);else if(type==='playback_changed'){const status=pick(event.status,event.playback?.status);if(status!==undefined)playing=isPlaying(status)}else if(type==='position_sync'||type==='position_changed')syncPosition(pick(event.position,event.position_ms));else if(event?.item)updatePlayback(event)}
function connect(url,onMessage,onOpen){let delay=600;function open(){let ws;try{ws=new WebSocket(url)}catch{return setTimeout(open,delay)}ws.onopen=()=>{delay=600;onOpen?.(ws)};ws.onmessage=e=>{try{onMessage(JSON.parse(e.data))}catch(err){console.warn('Invalid websocket event',err)}};ws.onclose=()=>{setTimeout(open,delay);delay=Math.min(15000,delay*1.6)};ws.onerror=()=>ws.close()}open()}
connect(config.soloistWebSocket||'ws://127.0.0.1:9090',onSoloist,ws=>ws.send(JSON.stringify({type:'command',command:'get_state'})));
if(config.visualizer!==false)connect(config.audioWebSocket||'ws://127.0.0.1:9091',event=>{if(event.type==='spectrum'&&Array.isArray(event.bars)){bars=event.bars.map(x=>Math.min(1,Math.max(0,Number(x)||0)));audioAt=performance.now()}});
function clock(){ $('clock').textContent=new Date().toLocaleTimeString(config.locale||undefined,{hour:'2-digit',minute:'2-digit',hour12:false})}clock();setInterval(clock,1000);
function frame(now){const dt=Math.min(.08,(now-lastFrame)/1000);lastFrame=now;const desired=playing?2*Math.PI/12:0;angularVelocity+=(desired-angularVelocity)*(1-Math.exp(-dt*(playing?6:4)));angle=(angle+angularVelocity*dt)%(2*Math.PI);rotor.style.transform=`rotate(${angle}rad)`;
const elapsed=playing?now-positionAt:0;const ratio=duration?Math.min(1,Math.max(0,(positionMs+elapsed)/duration)):0;fill.style.width=`${ratio*100}%`;progress.setAttribute('aria-valuenow',String(Math.round(ratio*100)));
const w=canvas.clientWidth,h=canvas.clientHeight,dpr=Math.min(devicePixelRatio||1,2);if(canvas.width!==Math.round(w*dpr)||canvas.height!==Math.round(h*dpr)){canvas.width=Math.round(w*dpr);canvas.height=Math.round(h*dpr)}ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,w,h);
const n=48,gap=Math.max(2,w*.004),bw=(w-gap*(n-1))/n;ctx.fillStyle=getComputedStyle(document.documentElement).getPropertyValue('--accent').trim();for(let i=0;i<n;i++){const target=now-audioAt>400?0:(bars[Math.floor(i*bars.length/n)]||0);smoothed[i]+=(target-smoothed[i])*(1-Math.exp(-dt*(target>smoothed[i]?24:8)));const bh=Math.max(2,smoothed[i]*h*.95);ctx.globalAlpha=.4+.6*smoothed[i];ctx.fillRect(i*(bw+gap),(h-bh)/2,bw,bh)}ctx.globalAlpha=1;requestAnimationFrame(frame)}requestAnimationFrame(frame);
