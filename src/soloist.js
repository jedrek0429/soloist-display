export function unwrap(value){return value?.entity||value}
export function entityName(value){const node=unwrap(value);return node?.decorations?.identity?.name||node?.name||node?.title||''}
export function artistFor(item){
  const explicit=item?.decorations?.creators||item?.artists||[];
  if(Array.isArray(explicit)&&explicit.length){const names=explicit.map(entityName).filter(Boolean);if(names.length)return names.join(', ')}
  let node=item;
  for(let i=0;i<8&&node;i++){
    node=unwrap(node);
    if(node?.entity_type==='artist')return entityName(node);
    node=unwrap(node?.parent);
  }
  return entityName(item?.artist)||'Unknown artist';
}
export function coverFor(item){
  const candidates=[item,item?.parent?.entity,item?.album].filter(Boolean);
  for(const node of candidates){
    const covers=node?.decorations?.visual_identity?.cover||node?.images||[];
    if(typeof covers==='string')return covers;
    if(!Array.isArray(covers))continue;
    const cover=covers.find(c=>c.size==='xlarge')||covers.find(c=>c.size==='large')||covers.find(c=>c.width>=500)||covers.at(-1);
    if(cover?.url)return cover.url;
  }
  return '';
}
// Prefer the album's stable identity; fall back to its cover when Soloist omits album IDs.
export function albumKeyFor(item){
  const candidates=[unwrap(item?.album),unwrap(item?.parent),unwrap(item?.release)];
  for(const album of candidates){
    if(!album)continue;
    const identity=album.uri||album.id||album.gid;
    if(identity)return 'album:'+String(identity);
  }
  const cover=coverFor(item);
  return cover?'cover:'+cover:null;
}
export function normalize(event){
  const item=event?.item||event?.playback?.item||event?.state?.item||null;
  const position=event?.position||event?.playback?.position||event?.state?.position||null;
  const status=event?.status??event?.playback?.status??event?.state?.status??null;
  return {item,position,status};
}
export function isEmptyPlayback(event){
  const {item,status}=normalize(event);
  if(item)return false;
  const explicitEmpty=[event,event?.playback,event?.state].some(node=>node&&Object.prototype.hasOwnProperty.call(node,'item')&&node.item===null);
  const stopped=['stopped','idle','none','not_playing'].includes(status)||['stopped','idle'].includes(status?.state);
  return explicitEmpty||stopped;
}
export function positionMs(position){if(typeof position==='number')return position;return Number(position?.position_ms??position?.position??position?.ms??0)}
export function durationMs(item){return Number(item?.decorations?.playback?.duration_ms??item?.duration_ms??0)}
export function playing(status){return status===true||status==='playing'||status==='play'||status?.state==='playing'}

export function pauseDelayMs(value){
  if(value==='never'||value==null)return null;
  const seconds=Number(value);
  return Number.isFinite(seconds)&&[0,30,60,300,600].includes(seconds)?seconds*1000:null;
}
