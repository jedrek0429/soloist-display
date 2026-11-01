const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const easing='cubic-bezier(.2,.75,.2,1)';
export class DiscDirector{
  constructor({discs,backdrops,info}){
    this.discs=discs;this.backdrops=backdrops;this.info=info;this.current=0;this.token=0;this.started=false;this.angle=0;this.velocity=0;this.playing=false;this.last=performance.now();
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.playing?Math.PI*2/12:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*5));
    this.angle=(this.angle+this.velocity*dt)%(Math.PI*2);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  async change(url,onShow){
    const token=++this.token;
    for(const disc of this.discs)disc.getAnimations().forEach(a=>a.cancel());
    this.info.getAnimations().forEach(a=>a.cancel());
    this.discs.forEach((disc,i)=>{disc.style.transform='';disc.style.opacity='';disc.classList.toggle('active',i===this.current)});
    this.info.classList.remove('revealing');
    const incoming=this.started?1-this.current:this.current;
    const outgoing=this.discs[this.current],next=this.discs[incoming];
    const image=next.querySelector('.disc-image');
    image.style.backgroundImage=url?'url('+JSON.stringify(url)+')':'none';
    const back=this.backdrops[incoming];back.style.backgroundImage=url?'url('+JSON.stringify(url)+')':'none';
    // Crossfade the blurred album background, not a generated colour.
    this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incoming));
    if(!this.started){
      this.started=true;next.classList.add('active');this.info.style.opacity='1';onShow();return;
    }
    this.info.style.opacity='0';
    next.classList.add('active');
    const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
    if(!reduced){
      const out=outgoing.animate([
        {transform:'translate(-50%,-50%) scale(1)',opacity:1},
        {transform:'translate(-155%,-50%) scale(.86)',opacity:1}
      ],{duration:540,easing:'cubic-bezier(.5,0,.75,.25)',fill:'forwards'});
      const incomingAnimation=next.animate([
        {transform:'translate(55%,-50%) scale(1.22)',opacity:1},
        {transform:'translate(-50%,-50%) scale(1.22)',opacity:1}
      ],{duration:780,easing,fill:'forwards'});
      try{await Promise.all([out.finished,incomingAnimation.finished])}catch{return}
      if(token!==this.token)return;
      outgoing.classList.remove('active');out.cancel();
      await wait(1000);if(token!==this.token)return;
      const settle=next.animate([
        {transform:'translate(-50%,-50%) scale(1.22)'},
        {transform:'translate(-50%,-50%) scale(1)'}
      ],{duration:680,easing,fill:'forwards'});
      try{await settle.finished}catch{return}
      if(token!==this.token)return;
      settle.cancel();incomingAnimation.cancel();
    }else{outgoing.classList.remove('active')}
    this.current=incoming;this.discs.forEach((disc,i)=>disc.classList.toggle('active',i===incoming));onShow();this.info.style.opacity='1';
    this.info.classList.remove('revealing');void this.info.offsetWidth;this.info.classList.add('revealing');
  }
}
