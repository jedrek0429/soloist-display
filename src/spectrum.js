export class Spectrum{
  constructor(canvas){this.canvas=canvas;this.ctx=canvas.getContext('2d');this.values=new Float32Array(32);this.target=new Float32Array(32);this.lastPacket=0;this.lastFrame=performance.now();requestAnimationFrame(t=>this.draw(t))}
  receive(bars){if(!Array.isArray(bars))return;for(let i=0;i<32;i++){this.target[i]=Math.max(0,Math.min(1,Number(bars[Math.floor(i*bars.length/32)])||0))}this.lastPacket=performance.now()}
  draw(now){
    const dt=Math.min(.08,(now-this.lastFrame)/1000);this.lastFrame=now;
    const canvas=this.canvas,w=canvas.clientWidth,h=canvas.clientHeight,dpr=Math.min(devicePixelRatio||1,2);
    if(canvas.width!==Math.round(w*dpr)||canvas.height!==Math.round(h*dpr)){canvas.width=Math.round(w*dpr);canvas.height=Math.round(h*dpr)}
    const ctx=this.ctx;ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,w,h);
    const gap=2,width=(w-31*gap)/32;
    ctx.fillStyle='#f3f1ed';
    for(let i=0;i<32;i++){
      const target=now-this.lastPacket>1000?0:this.target[i];
      this.values[i]+=(target-this.values[i])*(1-Math.exp(-dt*(target>this.values[i]?25:7)));
      const bh=Math.max(1.5,this.values[i]*h);
      ctx.globalAlpha=.55+.45*this.values[i];ctx.fillRect(i*(width+gap),h-bh,width,bh);
    }
    ctx.globalAlpha=1;requestAnimationFrame(t=>this.draw(t));
  }
}
