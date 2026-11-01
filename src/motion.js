const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const ease='cubic-bezier(.22,.75,.2,1)';
const imageStyle=url=>url?'url('+JSON.stringify(url)+')':'none';
function preload(url){
  return new Promise(resolve=>{
    if(!url){resolve();return}
    const img=new Image();let done=false;
    const finish=()=>{if(!done){done=true;resolve()}};
    img.onload=finish;img.onerror=finish;img.src=url;
    setTimeout(finish,1800);
  });
}
async function play(element,frames,options,token,owner){
  const animation=element.animate(frames,{...options,fill:'forwards'});
  try{await animation.finished}catch{return false}
  return owner.token===token;
}
export class DiscDirector{
  constructor({discs,backdrops,info}){
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;
    this.angle=0;this.velocity=0;this.playing=false;this.changing=false;
    this.last=performance.now();
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.changing?0:(this.playing?Math.PI*2/12:0);
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*(this.changing?11:5)));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  async change(url,onShow){
    const token=++this.token;
    const oldIndex=this.current,newIndex=this.started?1-oldIndex:oldIndex;
    const oldDisc=this.discs[oldIndex],newDisc=this.discs[newIndex];
    await preload(url);
    if(token!==this.token)return;
    // Reset any interrupted animations; retain the most recently committed disc.
    for(const disc of this.discs){
      disc.getAnimations().forEach(a=>a.cancel());
      disc.style.transform='';disc.style.opacity='';disc.style.visibility='';
    }
    newDisc.querySelector('.disc-image').style.backgroundImage=imageStyle(url);
    this.backdrops[newIndex].style.backgroundImage=imageStyle(url);
    if(!this.started){
      this.started=true;newDisc.classList.add('active');
      this.backdrops[newIndex].classList.add('active');onShow();return;
    }
    this.changing=true;
    // Disc exchange is the primary navigation language, not a decorative effect.
    const reduced=false;
    this.info.style.opacity='0';
    oldDisc.classList.add('active');
    newDisc.classList.remove('active');
    if(!reduced){
      // STOP: hold the record at the spindle long enough for the stop to read.
      await wait(380);
      if(token!==this.token)return;
      // LIFT: bring the disc towards the viewer and tilt it, then carry it out.
      if(!await play(oldDisc,[
        {transform:'translate(-50%,-50%) scale(1) rotateX(0deg)',offset:0},
        {transform:'translate(-50%,-59%) scale(.91) rotateX(38deg)',offset:.38},
        {transform:'translate(-135%,-85%) scale(.72) rotateX(65deg)',offset:1}
      ],{duration:1250,easing:'cubic-bezier(.4,0,.55,1)'},token,this))return;
      oldDisc.classList.remove('active');
      oldDisc.getAnimations().forEach(a=>a.cancel());
      if(token!==this.token)return;
      // REPLACE: second disc starts outside the viewport, visibly travels in and drops flat.
      newDisc.classList.add('active');
      const placement=play(newDisc,[
        {transform:'translate(40%,-105%) scale(.72) rotateX(65deg)',offset:0},
        {transform:'translate(-50%,-61%) scale(.91) rotateX(36deg)',offset:.64},
        {transform:'translate(-50%,-50%) scale(1) rotateX(0deg)',offset:1}
      ],{duration:1450,easing},token,this);
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===newIndex));
      if(!await placement)return;
      newDisc.getAnimations().forEach(a=>a.cancel());
      await wait(160);
      if(token!==this.token)return;
    }else{
      oldDisc.classList.remove('active');
      newDisc.classList.add('active');
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===newIndex));
    }
    this.current=newIndex;
    this.changing=false;this.velocity=0;
    onShow();
    this.info.style.opacity='1';
    this.info.classList.remove('revealing');void this.info.offsetWidth;this.info.classList.add('revealing');
  }
}
