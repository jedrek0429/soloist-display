const ease='cubic-bezier(.25,.7,.2,1)';
const wait=ms=>new Promise(r=>setTimeout(r,ms));
export class DiscDirector{
  constructor({discs,backdrops,info}){
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;this.angle=0;this.velocity=0;this.playing=false;this.last=performance.now();
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.playing?Math.PI*2/12:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*5));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  async change(url,onShow){
    const token=++this.token;
    // Interrupt a previous exchange without leaving hidden or offscreen discs.
    for(const disc of this.discs){disc.getAnimations().forEach(a=>a.cancel());disc.style.transform='';disc.style.opacity='';disc.style.visibility=''}
    this.info.getAnimations().forEach(a=>a.cancel());
    this.info.classList.remove('revealing');
    const incoming=this.started?1-this.current:this.current;
    const outgoing=this.discs[this.current],next=this.discs[incoming];
    next.querySelector('.disc-image').style.backgroundImage=url?'url('+JSON.stringify(url)+')':'none';
    this.backdrops[incoming].style.backgroundImage=url?'url('+JSON.stringify(url)+')':'none';
    if(!this.started){
      this.started=true;next.classList.add('active');this.backdrops[incoming].classList.add('active');
      this.info.style.opacity='1';onShow();return;
    }
    // Remove previous metadata, as if the disc is being physically taken off.
    this.info.style.opacity='0';
    this.discs.forEach((disc,i)=>disc.classList.toggle('active',i===this.current||i===incoming));
    // Place the replacement out of sight until the old disc has been lifted away.
    next.style.visibility='hidden';
    const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
    if(!reduced){
      const lift=outgoing.animate([
        {transform:'translate(-50%,-50%) scale(1) rotateY(0deg)',offset:0},
        {transform:'translate(-50%,-52%) scale(1.035) rotateY(-9deg)',offset:.28},
        {transform:'translate(-145%,-66%) scale(.9) rotateY(-24deg)',offset:1}
      ],{duration:780,easing:'cubic-bezier(.42,0,.68,.25)',fill:'forwards'});
      try{await lift.finished}catch{return}
      if(token!==this.token)return;
      outgoing.classList.remove('active');lift.cancel();
      next.style.visibility='visible';
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incoming));
      // New record is lowered into the spindle, not merely crossfaded.
      const place=next.animate([
        {transform:'translate(0%,-75%) scale(1.12) rotateY(20deg)',offset:0},
        {transform:'translate(-50%,-54%) scale(1.06) rotateY(7deg)',offset:.65},
        {transform:'translate(-50%,-50%) scale(1) rotateY(0deg)',offset:1}
      ],{duration:920,easing,fill:'forwards'});
      try{await place.finished}catch{return}
      if(token!==this.token)return;
      place.cancel();
      // The record comes to rest briefly, then accelerates into its normal rotation.
      this.velocity=0;
      await wait(350);
      if(token!==this.token)return;
    }else{
      outgoing.classList.remove('active');next.style.visibility='visible';
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incoming));
    }
    this.current=incoming;
    this.discs.forEach((disc,i)=>disc.classList.toggle('active',i===incoming));
    onShow();this.info.style.opacity='1';
    this.info.classList.remove('revealing');void this.info.offsetWidth;this.info.classList.add('revealing');
  }
}
