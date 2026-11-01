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
  constructor({discs,backdrops,info,settings={}}){
    this.settings={discRotationSeconds:12,transitionDuration:2500,backdropTransitionStyle:'fade',backdropLinked:true,backdropTransitionDuration:1850,transitionStyle:'fly',metadataAnimation:true,reducedMotion:false,...settings};
    this.motionEnabled=!this.settings.reducedMotion;
    this.metadataEnabled=this.settings.metadataAnimation&&!this.settings.reducedMotion;
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;this.angle=0;
    this.velocity=0;this.playing=false;this.changing=false;this.last=performance.now();
    this.pendingShow=null;this.fadeTimer=null;this.animations=new Set();
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const target=this.changing?0:this.playing?Math.PI*2/this.settings.discRotationSeconds:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*(this.changing?12:5)));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs)disc.querySelector('.rotor').style.transform=`rotate(${disc.dataset.angleOffset?this.angle-Number(disc.dataset.angleOffset):this.angle}rad)`;
    requestAnimationFrame(t=>this.spin(t));
  }
  setPlaying(value){this.playing=value}
  animate(element,keyframes,duration,token){
    if(duration<=0){Object.assign(element.style,keyframes.at(-1));return Promise.resolve(true)}
    return new Promise(resolve=>{
      const animation=element.animate(keyframes,{duration,easing:'ease-in-out',fill:'forwards'});
      this.animations.add(animation);
      const finish=()=>{
        this.animations.delete(animation);
        if(token===this.token){try{animation.commitStyles?.()}catch{}animation.cancel();resolve(true)}
        else{animation.cancel();resolve(false)}
      };
      // A timer rather than animation.finished avoids compositor stalls on CRT browsers.
      setTimeout(finish,duration+35);
    });
  }
  fadeBackdrop(outgoing,incoming,duration,token){
    if(duration<=0){incoming.style.opacity='1';return Promise.resolve(true)}
    return new Promise(resolve=>{
      // CSS crossfade keeps the old image fully opaque beneath the incoming one.
      // Avoid WAAPI commitStyles()/cancel(), which can expose a frame of the wrong image.
      incoming.style.transition='none';
      incoming.style.opacity='0';
      outgoing.style.transition='none';
      outgoing.style.opacity='1';
      void incoming.offsetWidth;
      requestAnimationFrame(()=>requestAnimationFrame(()=>{
        if(token!==this.token){resolve(false);return}
        incoming.style.transition=`opacity ${duration}ms ease-in-out`;
        incoming.style.opacity='1';
      }));
      setTimeout(()=>resolve(token===this.token),duration+60);
    });
  }
  async change(url,onShow){
    if(this.pendingShow){this.pendingShow();this.pendingShow=null}
    const token=++this.token;
    for(const animation of this.animations)animation.cancel();
    this.animations.clear();
    clearTimeout(this.fadeTimer);
    this.changing=false;
    this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
    await preload(url);
    if(token!==this.token)return;
    const outgoingIndex=this.current;
    const incomingIndex=this.started?1-outgoingIndex:outgoingIndex;
    const outgoing=this.discs[outgoingIndex],incoming=this.discs[incomingIndex];
    const previousBg=this.backdrops[outgoingIndex],nextBg=this.backdrops[incomingIndex];
    for(const disc of this.discs){disc.style.transform='';disc.style.opacity='';disc.classList.remove('active')}
    for(const bg of this.backdrops){bg.style.transition='none';bg.style.transform='';bg.style.opacity=bg===previousBg?'1':'0';bg.style.zIndex='';}
    incoming.querySelector('.disc-image').style.backgroundImage=imageStyle(url);
    incoming.querySelector('.disc-label').style.backgroundImage=imageStyle(url);
    incoming.dataset.angleOffset=String(this.angle);
    nextBg.style.backgroundImage=imageStyle(url);
    const commit=()=>{
      if(token!==this.token)return;
      this.current=incomingIndex;
      outgoing.classList.remove('active');
      incoming.classList.add('active');
      outgoing.style.transform='';outgoing.style.opacity='';
      incoming.style.transform='';incoming.style.opacity='';
      previousBg.classList.remove('active');nextBg.classList.add('active');
      previousBg.style.transform='';previousBg.style.transition='none';previousBg.style.opacity='0';
      nextBg.style.transform='';nextBg.style.transition='none';nextBg.style.opacity='1';
      this.changing=false;this.pendingShow=null;
      this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
      onShow();
    };
    if(!this.started){this.started=true;commit();return}
    const duration=this.motionEnabled?this.settings.transitionDuration:0;
    const discStyle=this.motionEnabled?this.settings.transitionStyle:'instant';
    const backdropStyle=this.motionEnabled?this.settings.backdropTransitionStyle:'instant';
    const backdropDuration=this.motionEnabled?(this.settings.backdropLinked?duration:this.settings.backdropTransitionDuration):0;
    this.changing=discStyle==='slide'||discStyle==='fly';
    this.pendingShow=commit;
    outgoing.classList.add('active');incoming.classList.add('active');
    previousBg.classList.add('active');nextBg.classList.add('active');
    // The outgoing backdrop stays underneath until the incoming image is fully opaque.
    previousBg.style.zIndex='1';nextBg.style.zIndex='2';
    if(discStyle==='fade')incoming.style.opacity='0';
    if(discStyle==='slide')incoming.style.transform='translate(65%,-50%)';
    if(discStyle==='fly')incoming.style.transform='translate(55%,-115%) scale(.72) rotateX(65deg)';
    if(backdropStyle==='fade')nextBg.style.opacity='0';
    // Ensure the initial frame is committed before starting the animations.
    void incoming.offsetWidth;
    const jobs=[];
    if(discStyle==='fade')jobs.push(this.animate(incoming,[{opacity:0},{opacity:1}],duration,token));
    if(discStyle==='slide'){
      jobs.push(this.animate(outgoing,[{transform:'translate(-50%,-50%)'},{transform:'translate(-165%,-50%)'}],duration,token));
      jobs.push(this.animate(incoming,[{transform:'translate(65%,-50%)'},{transform:'translate(-50%,-50%)'}],duration,token));
    }
    if(discStyle==='fly'){
      jobs.push(this.animate(outgoing,[{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'},{transform:'translate(-145%,-98%) scale(.72) rotateX(65deg)'}],duration*.5,token));
      jobs.push((async()=>{
        await delay(duration*.5);
        if(token!==this.token)return false;
        onShow();
        return this.animate(incoming,[{transform:'translate(55%,-115%) scale(.72) rotateX(65deg)'},{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'}],duration*.5,token);
      })());
    }
    if(backdropStyle==='fade'){
      jobs.push(this.fadeBackdrop(previousBg,nextBg,backdropDuration,token));
    }else{
      nextBg.style.opacity='1';
      previousBg.style.opacity='0';
    }
    if(discStyle==='instant'){
      // Disc changes immediately even when the backdrop continues fading.
      outgoing.classList.remove('active');incoming.classList.add('active');
      this.current=incomingIndex;onShow();
    }
    if(discStyle==='instant'&&backdropStyle==='instant'){commit();return}
    if(this.metadataEnabled&&discStyle!=='instant'){
      this.info.style.animationDuration=duration+'ms';
      this.info.classList.add('metadata-exiting');
    }
    await Promise.all(jobs);
    if(token===this.token)commit();
  }
}
