export class Spectrum{
  constructor(canvas){
    this.canvas=canvas;this.ctx=canvas.getContext('2d');this.values=new Float32Array(6);this.target=new Float32Array(6);
    this.lastPacket=0;this.lastFrame=performance.now();this.packets=0;
    requestAnimationFrame(t=>this.draw(t));
  }
  receive(bars){
    if(!Array.isArray(bars)||bars.length===0)return;
    // Downsample in the frequency domain; keep peaks rather than dropping most bins.
    for(let i=0;i<6;i++){
      const start=Math.floor(i*bars.length/6),end=Math.max(start+1,Math.floor((i+1)*bars.length/6));
      let peak=0;
      for(let j=start;j<end;j++)peak=Math.max(peak,Number(bars[j])||0);
      // FFT bridge magnitudes are typically far below 1; expand real signal dynamics.
      this.target[i]=Math.min(1,Math.max(0,Math.pow(peak*3.8,0.65)));
    }
    this.lastPacket=performance.now();this.packets++;
  }
  draw(now){
    const dt=Math.min(.08,(now-this.lastFrame)/1000);this.lastFrame=now;
    const canvas=this.canvas,w=canvas.clientWidth,h=canvas.clientHeight,dpr=Math.min(devicePixelRatio||1,2);
    if(canvas.width!==Math.round(w*dpr)||canvas.height!==Math.round(h*dpr)){canvas.width=Math.round(w*dpr);canvas.height=Math.round(h*dpr)}
    const ctx=this.ctx;ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,w,h);
    const gap=3,width=Math.max(1,(w-5*gap)/6);
    ctx.fillStyle='#f4f0e9';
    for(let i=0;i<6;i++){
      const target=now-this.lastPacket>2500?0:this.target[i];
      this.values[i]+=(target-this.values[i])*(1-Math.exp(-dt*(target>this.values[i]?16:5)));
      const bh=Math.max(2,this.values[i]*h);
      ctx.globalAlpha=.55+.45*this.values[i];
      ctx.fillRect(i*(width+gap),h-bh,Math.min(width,3),bh);
    }
    ctx.globalAlpha=1;requestAnimationFrame(t=>this.draw(t));
  }
}
