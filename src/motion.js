const ease='cubic-bezier(.22,.72,.18,1)';
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const urlStyle=url=>url?'url('+JSON.stringify(url)+')':'none';
function ready(url){return new Promise(resolve=>{if(!url)return resolve();const image=new Image();image.onload=image.onerror=resolve;image.src=url;setTimeout(resolve,1500)})}
export class DiscDirector{
  constructor({discs,backdrops,info}){
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;this.angle=0;this.velocity=0;this.playing=false;this.last=performance.now();
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.playing?2*Math.PI/12:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*5));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  async change(url,onShow){
    const token=++this.token;
    // Keep the currently visible disc in place until replacement artwork is ready.
    const outgoing=this.discs[this.current],incomingIndex=this.started?1-this.current:this.current;
    const incoming=this.discs[incomingIndex];
    await ready(url);
    if(token!==this.token)return;
    for(const disc of this.discs){disc.getAnimations().forEach(a=>a.cancel());disc.style.transform='';disc.style.opacity='';disc.style.visibility=''}
    incoming.querySelector('.disc-image').style.backgroundImage=urlStyle(url);
    this.backdrops[incomingIndex].style.backgroundImage=urlStyle(url);
    if(!this.started){
      this.started=true;incoming.classList.add('active');this.backdrops[incomingIndex].classList.add('active');onShow();return;
    }
    const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
    outgoing.classList.add('active');incoming.classList.remove('active');
    if(!reduced){
      // Phase 1: lift the old disc off the spindle and carry it out.
      const lift=outgoing.animate([
        {transform:'translate(-50%,-50%) scale(1) rotateY(0deg)'},
        {transform:'translate(-50%,-54%) scale(1.035) rotateY(-12deg)',offset:.3},
        {transform:'translate(-135%,-75%) scale(.83) rotateY(-32deg)'}
      ],{duration:820,easing:'cubic-bezier(.4,0,.7,.35)',fill:'forwards'});
      try{await lift.finished}catch{return}
      if(token!==this.token)return;
      // Phase 2: lower a different record onto the spindle.
      incoming.classList.add('active');
      const place=incoming.animate([
        {transform:'translate(12%,-84%) scale(1.13) rotateY(28deg)'},
        {transform:'translate(-46%,-58%) scale(1.04) rotateY(9deg)',offset:.72},
        {transform:'translate(-50%,-50%) scale(1) rotateY(0deg)'}
      ],{duration:1000,easing,fill:'forwards'});
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incomingIndex));
      try{await place.finished}catch{return}
      if(token!==this.token)return;
      // Retain the final image before cancelling fill animations.
      outgoing.classList.remove('active');
      lift.cancel();place.cancel();
      this.velocity=0;
      await wait(250);if(token!==this.token)return;
    }else{
      outgoing.classList.remove('active');incoming.classList.add('active');
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incomingIndex));
    }
    this.current=incomingIndex;
    onShow();
    this.info.classList.remove('revealing');void this.info.offsetWidth;this.info.classList.add('revealing');
  }
}
