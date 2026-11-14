// Perceptual backdrop colour extraction. Kept independent of the display so it can be tested.
const clamp=(value,lo,hi)=>Math.max(lo,Math.min(hi,value));
const linear=value=>{
  const x=value/255;
  return x<=.04045?x/12.92:((x+.055)/1.055)**2.4;
};
const srgb=value=>value<=.0031308?12.92*value:1.055*Math.pow(value,1/2.4)-.055;

function toOklab(r,g,b){
  const red=linear(r),green=linear(g),blue=linear(b);
  const l=Math.cbrt(.4122214708*red+.5363325363*green+.0514459929*blue);
  const m=Math.cbrt(.2119034982*red+.6806995451*green+.1073969566*blue);
  const s=Math.cbrt(.0883024619*red+.2817188376*green+.6299787005*blue);
  return [
    .2104542553*l+.7936177850*m-.0040720468*s,
    1.9779984951*l-2.4285922050*m+.4505937099*s,
    .0259040371*l+.7827717662*m-.8086757660*s
  ];
}

function fromOklch(lightness,chroma,hue){
  // Retain the intended hue while fitting highly saturated colours into sRGB.
  for(let attempt=0;attempt<24;attempt++){
    const a=chroma*Math.cos(hue),b=chroma*Math.sin(hue);
    const ll=(lightness+.3963377774*a+.2158037573*b)**3;
    const mm=(lightness-.1055613458*a-.0638541728*b)**3;
    const ss=(lightness-.0894841775*a-1.2914855480*b)**3;
    const rgb=[
      4.0767416621*ll-3.3077115913*mm+.2309699292*ss,
      -1.2684380046*ll+2.6097574011*mm-.3413193965*ss,
      -.0041960863*ll-.7034186147*mm+1.7076147010*ss
    ];
    if(rgb.every(value=>value>=0&&value<=1)){
      return '#'+rgb.map(value=>Math.round(clamp(srgb(value),0,1)*255).toString(16).padStart(2,'0')).join('');
    }
    chroma*=.9;
  }
  return '#555963';
}

/**
 * Find a prominent *chromatic region*, not the arithmetic mean of the pixels.
 * White typography, black borders and small neon accents must not overwhelm
 * the colour of the album. OKLab avoids RGB averaging's muddy results.
 */
export function dominantColourFromPixels({data,width,height}){
  if(!data||!width||!height)return null;
  const bins=Array.from({length:24},()=>({weight:0,pixels:0,a:0,b:0,l:0,c:0}));
  let visible=0,neutralLightness=0,neutralWeight=0;
  const stride=Math.max(1,Math.floor(Math.max(width,height)/64));
  for(let y=0;y<height;y+=stride){
    for(let x=0;x<width;x+=stride){
      const i=(y*width+x)*4;
      const alpha=data[i+3]/255;
      if(alpha<.5)continue;
      const [l,a,b]=toOklab(data[i],data[i+1],data[i+2]);
      const chroma=Math.hypot(a,b);
      visible+=alpha;
      if(l>.12&&l<.94){
        neutralLightness+=l*alpha;
        neutralWeight+=alpha;
      }
      // Low-contrast grayscale and near-black/white regions make poor accent colours.
      if(l<.14||l>.93||chroma<.027)continue;
      const hue=Math.atan2(b,a);
      const index=Math.floor((hue+Math.PI)/(2*Math.PI)*bins.length)%bins.length;
      const weight=alpha*(.8+.65*Math.min(chroma/.12,1.5));
      const bucket=bins[index];
      bucket.weight+=weight;
      bucket.pixels+=alpha;
      bucket.a+=a*weight;
      bucket.b+=b*weight;
      bucket.l+=l*weight;
      bucket.c+=chroma*weight;
    }
  }
  if(!visible)return null;
  let winner=null;
  for(let i=0;i<bins.length;i++){
    const group={weight:0,pixels:0,a:0,b:0,l:0,c:0};
    for(const offset of [-1,0,1]){
      const bucket=bins[(i+offset+bins.length)%bins.length];
      for(const key of Object.keys(group))group[key]+=bucket[key];
    }
    if(group.pixels<visible*.012)continue;
    const avgChroma=group.c/group.weight;
    const score=group.weight*(.8+.2*Math.min(avgChroma/.12,1.5));
    if(!winner||score>winner.score)winner={...group,score};
  }
  if(!winner){
    // On genuinely monochrome artwork a restrained cool neutral is preferable
    // to inventing a saturated accent that the original art does not contain.
    const avg=neutralWeight?neutralLightness/neutralWeight:.48;
    return fromOklch(clamp(.54+(avg-.5)*.08,.5,.58),.022,4.35);
  }
  const hue=Math.atan2(winner.b,winner.a);
  const lightness=clamp(.58+(winner.l/winner.weight-.5)*.12,.54,.63);
  const chroma=clamp(winner.c/winner.weight*1.3,.095,.19);
  return fromOklch(lightness,chroma,hue);
}

// Cache tiny derived colours, not decoded bitmaps.
const cache=new Map();
export function dominantColourForArtwork(url,image){
  if(!url||!image)return null;
  if(cache.has(url))return cache.get(url);
  let colour=null;
  try{
    const canvas=document.createElement('canvas');
    canvas.width=canvas.height=64;
    const context=canvas.getContext('2d',{willReadFrequently:true});
    if(context){
      context.drawImage(image,0,0,64,64);
      colour=dominantColourFromPixels(context.getImageData(0,0,64,64));
    }
  }catch{
    // A failed pixel read still allows an unchanged, blurred CSS artwork fallback.
  }
  cache.set(url,colour);
  if(cache.size>64)cache.delete(cache.keys().next().value);
  return colour;
}
