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
    this.settings={discRotationSeconds:12,transitionDuration:2000,backdropTransitionStyle:'fade',backdropTransitionDuration:2000,transitionStyle:'fly',sleeveTransitionStyle:'in-out',sleeveDuration:1000,sleeveDelayIn:0,sleeveDelayOut:950,transitionDelay:1000,backdropTransitionDelay:1000,panelTransitionDelay:0,panelDelayIn:0,panelDelayOut:0,sleeveAlignment:'center',panelTransitionStyle:'slide',panelTransitionDuration:2050,reducedMotion:false,pauseRetractMs:null,...settings};
    this.motionEnabled=!this.settings.reducedMotion;
    this.discs=discs;this.backdrops=backdrops;this.info=info;
    this.current=0;this.token=0;this.started=false;this.angle=0;
    this.velocity=0;this.playing=false;this.changing=false;this.last=performance.now();
    this.pendingShow=null;this.fadeTimer=null;this.animations=new Set();this.panelAnimation=null;this.panelToken=0;
    this.pauseRetractTimer=null;this.pauseStartedAt=null;this.pauseRetracted=false;this.artworkChanging=false;this.playbackKnown=false;
    requestAnimationFrame(t=>this.spin(t));
  }
  spin(now){
    const dt=Math.min(.08,(now-this.last)/1000);this.last=now;
    const appearance=document.documentElement.dataset.discAppearance;
    const sleeve=['vinyl-cover','cd-cover'].includes(appearance);
    const target=appearance==='album-cover'||(this.changing&&!sleeve)?0:this.playing?Math.PI*2/this.settings.discRotationSeconds:0;
    this.velocity+=(target-this.velocity)*(1-Math.exp(-dt*(this.changing?12:5)));
    this.angle=(this.angle+this.velocity*dt)%(2*Math.PI);
    for(const disc of this.discs){if(!disc.classList.contains('active')||appearance==='album-cover')continue;disc.querySelector('.rotor').style.transform=`rotate(${disc.dataset.angleOffset?this.angle-Number(disc.dataset.angleOffset):this.angle}rad)`;}
    requestAnimationFrame(t=>this.spin(t));
  }
  reset(){
    this.clearPauseRetraction();
    this.artworkChanging=false;
    this.wasRetractedAtChange=false;
    this.playbackKnown=false;
    ++this.token;
    ++this.panelToken;
    for(const animation of this.animations)animation.cancel();
    this.animations.clear();
    this.panelAnimation?.cancel();
    this.panelAnimation=null;
    clearTimeout(this.fadeTimer);
    this.pendingShow=null;
    this.started=false;
    this.changing=false;
    this.playing=false;
    for(const disc of this.discs){
      disc.classList.remove('active');
      disc.style.transform='';
      disc.style.opacity='';
      disc.querySelector('.record-track').style.transform='';
      disc.querySelector('.record-track').style.transition='';
      disc.querySelector('.record-track').style.translate='';
      disc.querySelector('.record-sleeve').style.translate='';
    }
    for(const backdrop of this.backdrops){
      backdrop.classList.remove('active');
      backdrop.style.transition='none';
      backdrop.style.opacity='0';
      backdrop.style.zIndex='';
    }
    this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
    this.info.style.transform='';
    this.info.style.opacity='';
  }
  isSleeve(){
    return ['vinyl-cover','cd-cover'].includes(document.documentElement.dataset.discAppearance);
  }
  activeTrack(){
    return this.discs[this.current].querySelector('.record-track');
  }
  clearPauseRetraction(){
    clearTimeout(this.pauseRetractTimer);
    this.pauseRetractTimer=null;
    this.pauseStartedAt=null;
    this.pauseRetracted=false;
  }
  retractPausedTrack(){
    if(this.playing||!this.isSleeve()||!this.started||this.artworkChanging||this.pauseRetracted)return;
    this.pauseRetracted=true;
    const track=this.activeTrack();
    track.style.transition=this.motionEnabled?'transform 450ms ease-in-out':'none';
    track.style.transform='translateX(0)';
  }
  schedulePauseRetraction(){
    if(!this.isSleeve()||this.settings.pauseRetractMs===null||this.pauseStartedAt===null)return;
    clearTimeout(this.pauseRetractTimer);
    this.pauseRetractTimer=null;
    const remaining=Math.max(0,this.settings.pauseRetractMs-(Date.now()-this.pauseStartedAt));
    if(remaining===0){this.retractPausedTrack();return;}
    this.pauseRetractTimer=setTimeout(()=>{
      this.pauseRetractTimer=null;
      this.retractPausedTrack();
    },remaining);
  }
  setPlaying(value){
    this.playing=value;
    if(!value&&!this.started&&!this.artworkChanging)return;
    this.playbackKnown=true;
    if(value){
      clearTimeout(this.pauseRetractTimer);
      this.pauseRetractTimer=null;
      this.pauseStartedAt=null;
      if(this.pauseRetracted&&this.isSleeve()&&this.started&&!this.artworkChanging){
        const track=this.activeTrack();
        track.style.transition=this.motionEnabled?'transform 450ms ease-in-out':'none';
        track.style.transform='translateX(50%)';
      }
      this.pauseRetracted=false;
      return;
    }
    if(this.pauseStartedAt===null){
      this.pauseStartedAt=Date.now();
      this.schedulePauseRetraction();
    }
  }
  async enterFirst(incoming,backdrop,onShow,token){
    const style=this.motionEnabled?this.settings.transitionStyle:'off';
    const duration=this.motionEnabled?this.settings.transitionDuration:0;
    const wait=this.motionEnabled?this.settings.transitionDelay:0;
    const from=style==='fly'
      ?'translate(55%,-115%) scale(.72) rotateX(65deg)'
      :'translate(65%,-50%)';
    const to='translate(-50%,-50%)';
    this.started=true;
    this.changing=style==='fly'||style==='slide';
    if(style==='fade')incoming.style.opacity='0';
    if(style==='fly'||style==='slide')incoming.style.transform=from;
    incoming.classList.add('active');
    backdrop.classList.add('active');
    backdrop.style.opacity='1';
    onShow();
    if(style!=='off'){
      if(wait>0)await delay(wait);
      if(token!==this.token)return;
      if(style==='fade')await this.animate(incoming,[{opacity:0},{opacity:1}],duration,token);
      else await this.animate(incoming,[{transform:from},{transform:to}],duration,token);
    }
    if(token!==this.token)return;
    incoming.style.transform='';
    incoming.style.opacity='';
    this.changing=false;
  }
  async after(wait,token,work){
    if(wait>0)await delay(wait);
    if(token!==this.token)return false;
    return work();
  }
  animate(element,keyframes,duration,token){
    if(duration<=0){Object.assign(element.style,keyframes.at(-1));return Promise.resolve(true)}
    return new Promise(resolve=>{
      const animation=element.animate(keyframes,{duration,easing:'ease-in-out',fill:'forwards'});
      this.animations.add(animation);
      const finish=()=>{
        this.animations.delete(animation);
        if(token===this.token){Object.assign(element.style,keyframes.at(-1));animation.cancel();resolve(true)}
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
  async transitionPanel(show){
    const token=++this.panelToken;
    if(this.settings.panelEnabled===false){show();return;}
    this.panelAnimation?.cancel();
    this.panelAnimation=null;
    this.info.classList.remove('metadata-exiting','metadata-entering','revealing');
    this.info.style.transform='';
    this.info.style.opacity='';
    const style=this.motionEnabled?this.settings.panelTransitionStyle:'off';
    const duration=this.motionEnabled?this.settings.panelTransitionDuration:0;
    if(style==='off'||duration<=0){show();return}
    const wait=this.motionEnabled?this.settings.panelDelayIn:0;
    if(wait>0)await delay(wait);
    if(token!==this.panelToken)return;
    const half=duration/2;
    const distance='calc(100% + 6vh + 60px)';
    const exit=style==='slide'
      ?[{transform:'translateY(0)',opacity:1},{transform:`translateY(${distance})`,opacity:1}]
      :[{opacity:1},{opacity:0}];
    const enter=style==='slide'
      ?[{transform:`translateY(${distance})`,opacity:1},{transform:'translateY(0)',opacity:1}]
      :[{opacity:0},{opacity:1}];
    const run=frames=>new Promise(resolve=>{
      const animation=this.info.animate(frames,{duration:half,easing:'ease-in-out',fill:'forwards'});
      this.panelAnimation=animation;
      animation.onfinish=()=>resolve(true);
      animation.oncancel=()=>resolve(false);
    });
    if(!await run(exit)||token!==this.panelToken)return;
    // Swap the text only after the old panel has fully left the screen.
    show();
    const outWait=this.motionEnabled?this.settings.panelDelayOut:0;
    if(outWait>0)await delay(outWait);
    if(token!==this.panelToken)return;
    this.panelAnimation?.cancel();
    await run(enter);
    if(token!==this.panelToken)return;
    this.panelAnimation?.cancel();
    this.panelAnimation=null;
    this.info.style.transform='';
    this.info.style.opacity='';
  }
  changePanel(onShow){
    // Leave disc, backdrop, rotation and pending disc transitions untouched.
    void this.transitionPanel(onShow);
  }
  async changeSleeve(url,onShow,token){
    const outgoingIndex=this.current;
    const incomingIndex=this.started?1-outgoingIndex:outgoingIndex;
    const outgoing=this.discs[outgoingIndex],incoming=this.discs[incomingIndex];
    const previousBg=this.backdrops[outgoingIndex],nextBg=this.backdrops[incomingIndex];
    const oldTrack=outgoing.querySelector('.record-track');
    const newTrack=incoming.querySelector('.record-track');
    const oldSleeve=outgoing.querySelector('.record-sleeve');
    const newSleeve=incoming.querySelector('.record-sleeve');
    const sleeveOn=this.motionEnabled&&this.settings.sleeveTransitionStyle!=='off';
    const sleeveDuration=sleeveOn?this.settings.sleeveDuration:0;
    // In: retract the outgoing record; Out: reveal the incoming record.
    const sleeveDelayIn=sleeveOn?this.settings.sleeveDelayIn:0;
    const sleeveDelayOut=sleeveOn?this.settings.sleeveDelayOut:0;
    const align=this.settings.sleeveAlignment==='center';
    // Individual translate is independent of the track's sliding transform.
        const offset=align?'25%':'0%';

    for(const disc of this.discs){
      disc.style.transform='';disc.style.opacity='';
      disc.querySelector('.record-track').style.translate='0% 0';
      disc.querySelector('.record-track').style.transition='none';
      disc.querySelector('.record-sleeve').style.translate='0% 0';
    }
    for(const bg of this.backdrops){
      bg.style.transition='none';bg.style.opacity=bg===previousBg?'1':'0';bg.style.zIndex='';
    }
    incoming.querySelector('.disc-label').style.backgroundImage=imageStyle(url);
    incoming.style.setProperty('--cd-cover-art',imageStyle(url));
    newSleeve.style.backgroundImage=imageStyle(url);
    nextBg.style.backgroundImage=imageStyle(url);
    incoming.dataset.angleOffset=String(this.angle);
    newTrack.style.transform=sleeveOn&&this.started?'translateX(0)':'translateX(50%)';
    if(!this.started)oldTrack.style.transform='translateX(50%)';

    const commit=()=>{
      if(token!==this.token)return;
      this.current=incomingIndex;
      outgoing.classList.remove('active');
      incoming.classList.add('active');
      incoming.style.transform='';incoming.style.opacity='';
      outgoing.style.transform='';outgoing.style.opacity='';
      // Keep the completed track exactly at the animated position.
      newTrack.style.transform='translateX(50%)';
      newTrack.style.translate='0% 0';newSleeve.style.translate='0% 0';
      previousBg.classList.remove('active');
      nextBg.classList.add('active');
      previousBg.style.opacity='0';nextBg.style.opacity='1';
      previousBg.style.transition='none';nextBg.style.transition='none';
      previousBg.style.zIndex='';nextBg.style.zIndex='';
      this.changing=false;
      this.artworkChanging=false;
      if(!this.playing)this.schedulePauseRetraction();
    };
    if(!this.started){
      await this.enterFirst(incoming,nextBg,onShow,token);
      if(token===this.token)commit();
      return;
    }
    void this.transitionPanel(onShow);
    const duration=this.motionEnabled?this.settings.transitionDuration:0;
    const style=this.motionEnabled?this.settings.transitionStyle:'off';
    const bgDuration=this.motionEnabled&&this.settings.backdropTransitionStyle==='fade'?this.settings.backdropTransitionDuration:0;
    this.changing=style==='slide'||style==='fly';
    outgoing.classList.add('active');incoming.classList.add('active');
    previousBg.classList.add('active');nextBg.classList.add('active');
    previousBg.style.zIndex='1';nextBg.style.zIndex='2';

    const jobs=[];
    if(sleeveOn){
      if(!this.wasRetractedAtChange){
        jobs.push(this.after(sleeveDelayIn,token,()=>this.animate(oldTrack,[{transform:'translateX(50%)'},{transform:'translateX(0)'}],sleeveDuration/2,token)));
      }
      if(align){
        if(!this.wasRetractedAtChange){
          jobs.push(this.after(sleeveDelayIn,token,()=>this.animate(oldTrack,[{translate:'0% 0'},{translate:offset+' 0'}],sleeveDuration/2,token)));
          jobs.push(this.after(sleeveDelayIn,token,()=>this.animate(oldSleeve,[{translate:'0% 0'},{translate:offset+' 0'}],sleeveDuration/2,token)));
        }
        newTrack.style.translate=offset+' 0';newSleeve.style.translate=offset+' 0';
      }
      jobs.push((async()=>{
        // The incoming record must not emerge before the outgoing one is tucked away.
        // Its own delay begins after that point (or the incoming disc's arrival).
        const discArrival=this.motionEnabled&&['fly','slide'].includes(this.settings.transitionStyle)
          ?this.settings.transitionDelay+this.settings.transitionDuration/2:0;
        await delay(Math.max(sleeveDelayIn+sleeveDuration/2,discArrival)+sleeveDelayOut);
        if(token!==this.token)return false;
        const opening=[this.animate(newTrack,[{transform:'translateX(0)'},{transform:'translateX(50%)'}],sleeveDuration/2,token)];
        if(align){
          opening.push(this.animate(newTrack,[{translate:offset+' 0'},{translate:'0% 0'}],sleeveDuration/2,token));
          opening.push(this.animate(newSleeve,[{translate:offset+' 0'},{translate:'0% 0'}],sleeveDuration/2,token));
        }
        return Promise.all(opening);
      })());
    }
    if(style==='fade'){
      incoming.style.opacity='0';
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(incoming,[{opacity:0},{opacity:1}],duration,token)));
    }
    if(style==='slide'){
      incoming.style.transform='translate(65%,-50%)';
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(outgoing,[{transform:'translate(-50%,-50%)'},{transform:'translate(-165%,-50%)'}],duration,token)));
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(incoming,[{transform:'translate(65%,-50%)'},{transform:'translate(-50%,-50%)'}],duration,token)));
    }
    if(style==='fly'){
      incoming.style.transform='translate(55%,-115%) scale(.72) rotateX(65deg)';
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(outgoing,[{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'},{transform:'translate(-145%,-98%) scale(.72) rotateX(65deg)'}],duration/2,token)));
      jobs.push((async()=>{
        await delay(this.settings.transitionDelay+duration/2);
        if(token!==this.token)return false;
        return this.animate(incoming,[{transform:'translate(55%,-115%) scale(.72) rotateX(65deg)'},{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'}],duration/2,token);
      })());
    }
    if(bgDuration)jobs.push(this.after(this.settings.backdropTransitionDelay,token,()=>this.fadeBackdrop(previousBg,nextBg,bgDuration,token)));
    else{nextBg.style.opacity='1';previousBg.style.opacity='0'}
    await Promise.all(jobs);
    if(token===this.token)commit();
  }
  async change(url,onShow){
    this.pendingShow=null;
    for(const overlay of document.querySelectorAll('.cover-overlay'))overlay.remove();
    const token=++this.token;
    for(const animation of this.animations)animation.cancel();
    this.animations.clear();
    clearTimeout(this.fadeTimer);
    this.changing=false;
    this.artworkChanging=true;
    this.wasRetractedAtChange=this.pauseRetracted;
    clearTimeout(this.pauseRetractTimer);
    this.pauseRetractTimer=null;
    this.pauseRetracted=false;
    if(this.playbackKnown&&!this.playing&&this.isSleeve())this.pauseStartedAt=Date.now();
    else this.pauseStartedAt=null;
    await preload(url);
    if(token!==this.token)return;
    if(['vinyl-cover','cd-cover'].includes(document.documentElement.dataset.discAppearance)){
      await this.changeSleeve(url,onShow,token);
      return;
    }
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
      this.artworkChanging=false;
      if(!this.playing)this.schedulePauseRetraction();
    };
    if(!this.started){
      await this.enterFirst(incoming,nextBg,onShow,token);
      if(token===this.token)commit();
      return;
    }
    // Panel timing is independent of the disc and backdrop transitions.
    void this.transitionPanel(onShow);
    const duration=this.motionEnabled?this.settings.transitionDuration:0;
    const discStyle=this.motionEnabled?this.settings.transitionStyle:'off';
    const backdropStyle=this.motionEnabled?this.settings.backdropTransitionStyle:'off';
    const backdropDuration=this.motionEnabled?(this.settings.backdropTransitionDuration):0;
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
    if(discStyle==='fade')jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(incoming,[{opacity:0},{opacity:1}],duration,token)));
    if(discStyle==='slide'){
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(outgoing,[{transform:'translate(-50%,-50%)'},{transform:'translate(-165%,-50%)'}],duration,token)));
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(incoming,[{transform:'translate(65%,-50%)'},{transform:'translate(-50%,-50%)'}],duration,token)));
    }
    if(discStyle==='fly'){
      jobs.push(this.after(this.settings.transitionDelay,token,()=>this.animate(outgoing,[{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'},{transform:'translate(-145%,-98%) scale(.72) rotateX(65deg)'}],duration*.5,token)));
      jobs.push((async()=>{
        await delay(this.settings.transitionDelay+duration*.5);
        if(token!==this.token)return false;
        return this.animate(incoming,[{transform:'translate(55%,-115%) scale(.72) rotateX(65deg)'},{transform:'translate(-50%,-50%) scale(1) rotateX(0deg)'}],duration*.5,token);
      })());
    }
    if(backdropStyle==='fade'){
      jobs.push(this.after(this.settings.backdropTransitionDelay,token,()=>this.fadeBackdrop(previousBg,nextBg,backdropDuration,token)));
    }else{
      nextBg.style.opacity='1';
      previousBg.style.opacity='0';
    }
    if(discStyle==='off'){
      // Disc changes immediately even when the backdrop continues fading.
      outgoing.classList.remove('active');incoming.classList.add('active');
      this.current=incomingIndex;
    }
    if(discStyle==='off'&&backdropStyle==='off'){commit();return}
    await Promise.all(jobs);
    if(token===this.token)commit();
  }
}
