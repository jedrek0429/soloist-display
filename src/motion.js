const imageStyle=url=>url?'url('+JSON.stringify(url)+')':'none';
function preload(url){
  return new Promise(resolve=>{
    if(!url)return resolve();
    const img=new Image();let finished=false;
    const done=()=>{if(!finished){finished=true;resolve()}};
    img.onload=done;img.onerror=done;img.src=url;setTimeout(done,1500);
  });
}
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
export class DiscDirector{
  constructor({discs,backdrops,info}){
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;this.angle=0;
    this.velocity=0;this.playing=false;this.changing=false;this.last=performance.now();
    this.pendingShow=null;this.fadeTimer=null;
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.changing?0:this.playing?Math.PI*2/12:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*(this.changing?12:5)));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  // Explicit frame-based transforms avoid Web Animations cancellation/finish races
  // and ensure each track change commits the correct current disc.
  animateDisc(disc,frames,duration,token){
    return new Promise(resolve=>{
      const start=performance.now();
      let finished=false;
      const finish=ok=>{
        if(finished)return;
        finished=true;
        clearTimeout(watchdog);
        resolve(ok);
      };
      // A throttled or interrupted Firefox animation must never strand the disc.
      const watchdog=setTimeout(()=>{
        if(token===this.token){
          const last=frames[frames.length-1];
          disc.style.transform=`translate(${last[0]}%,${last[1]}%) scale(${last[2]}) rotateX(${last[3]}deg)`;
        }
        finish(token===this.token);
      },duration+750);
      const tick=now=>{
        if(finished)return;
        if(token!==this.token){finish(false);return}
        const t=Math.min(1,(now-start)/duration);
        const eased=t<.5?4*t*t*t:1-Math.pow(-2*t+2,3)/2;
        const index=Math.min(frames.length-2,Math.floor(eased*(frames.length-1)));
        const local=eased*(frames.length-1)-index;
        const a=frames[index],b=frames[index+1];
        const lerp=(x,y)=>x+(y-x)*local;
        disc.style.transform=`translate(${lerp(a[0],b[0])}%,${lerp(a[1],b[1])}%) scale(${lerp(a[2],b[2])}) rotateX(${lerp(a[3],b[3])}deg)`;
        if(t<1)requestAnimationFrame(tick);else finish(true);
      };
      requestAnimationFrame(tick);
    });
  }
  async change(url,onShow){
    const token=++this.token;
    // An interrupted exchange must settle before a new one starts.
    if(this.pendingShow){
      this.pendingShow();
      this.pendingShow=null;
    }
    this.changing=false;
    this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
    clearTimeout(this.fadeTimer);
    await preload(url);
    if(token!==this.token)return;
    // Commit the previous track even if its choreography was interrupted.
    const outgoingIndex=this.current;
    const incomingIndex=this.started?1-outgoingIndex:outgoingIndex;
    const outgoing=this.discs[outgoingIndex],incoming=this.discs[incomingIndex];
    for(const disc of this.discs){disc.style.transform='';disc.classList.remove('active')}
    for(const bg of this.backdrops){bg.style.transition='none';bg.style.opacity='';bg.style.zIndex='';}
    incoming.querySelector('.disc-image').style.backgroundImage=imageStyle(url);
    this.backdrops[incomingIndex].style.backgroundImage=imageStyle(url);
    if(!this.started){
      this.started=true;this.current=incomingIndex;
      incoming.classList.add('active');
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incomingIndex));
      onShow();return;
    }
    this.changing=true;
    // Commit the new album even if the next track interrupts the choreography.
    this.pendingShow=()=>{
      this.current=incomingIndex;
      incoming.style.transform='';
      outgoing.style.transform='';
      outgoing.classList.remove('active');
      incoming.classList.add('active');
      this.backdrops.forEach((b,i)=>b.classList.toggle('active',i===incomingIndex));
      this.changing=false;
      this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
      onShow();
    };
    outgoing.classList.add('active');
    this.info.classList.remove('metadata-entering','revealing');
    try{
      // Keep outgoing artwork underneath while the new artwork slowly fades in.
      const previousBg=this.backdrops[outgoingIndex];
      const nextBg=this.backdrops[incomingIndex];
      previousBg.style.zIndex='1';
      nextBg.style.zIndex='2';
      nextBg.style.transition='none';
      nextBg.style.opacity='0';
      nextBg.classList.remove('active');
      void nextBg.offsetWidth;
      requestAnimationFrame(()=>requestAnimationFrame(()=>{
        if(token!==this.token)return;
        nextBg.style.transition='opacity 1850ms ease-in-out';
        nextBg.style.opacity='1';
      }));
      this.fadeTimer=setTimeout(()=>{
        if(token!==this.token)return;
        previousBg.classList.remove('active');
        previousBg.style.opacity='0';
        nextBg.classList.add('active');
        nextBg.style.transition='';
        nextBg.style.opacity='';
      },2050);

      await delay(220);
      if(token!==this.token)return;
      this.info.classList.add('metadata-exiting');
      const lifted=await this.animateDisc(outgoing,[
        [-50,-50,1,0],[-50,-65,.88,43],[-145,-98,.72,65]
      ],1050,token);
      if(!lifted)return;
      outgoing.classList.remove('active');
      incoming.classList.add('active');
      // Set the initial pose before the first animation frame: no artwork flash.
      incoming.style.transform='translate(55%,-115%) scale(.72) rotateX(65deg)';
      // Replace the copy off-screen, then bring the new card in with the disc.
      this.info.classList.remove('metadata-exiting');
      onShow();
      this.info.classList.add('metadata-entering');
      const landed=await this.animateDisc(incoming,[
        [55,-115,.72,65],[-50,-64,.9,30],[-50,-50,1,0]
      ],1250,token);
      if(!landed)return;
    }finally{
      // Never leave metadata hidden or rotation stopped, including interrupted transitions.
      if(token===this.token){
        incoming.style.transform='';
        outgoing.style.transform='';
        outgoing.classList.remove('active');
        incoming.classList.add('active');
        this.current=incomingIndex;
        this.changing=false;
        this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
        this.pendingShow=null;
        onShow();
      }
    }
  }
}
