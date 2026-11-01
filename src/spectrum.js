// Rendering only. Frequency analysis and smoothing happen in the Rust bridge.
export class Spectrum {
  constructor(canvas, sensitivity = 1, smoothing = 0.7, options = {}) {
    this.canvas = canvas;
    const gain = Number(sensitivity);
    this.sensitivity = Number.isFinite(gain) && gain >= 0 ? Math.min(gain, 10) : 1;
    this.smoothing = Math.max(0, Math.min(1, Number.isFinite(Number(smoothing)) ? Number(smoothing) : 0.7));
    this.ctx = canvas.getContext('2d');
    this.count = 6;
    this.autoNormalize = options.autoNormalize === true;
    this.peakHold = options.peakHold === true;
    this.targets = new Float32Array(this.count);
    this.values = new Float32Array(this.count);
    this.peaks = new Float32Array(this.count);
    this.peakTimes = new Float64Array(this.count);
    this.envelope = new Float32Array(this.count).fill(0.12);
    this.lastPacket = 0;
    this.lastFrame = performance.now();
    requestAnimationFrame(t => this.draw(t));
  }
  receive(packet) {
    if (!packet || !Array.isArray(packet.bands) || packet.bands.length < 6) return;
    for (let i = 0; i < this.count; i++) {
      const value=Math.max(0,Number(packet.bands[i]));
      if(!Number.isFinite(value))continue;
      // Slowly adapt to the programme level, without amplifying digital silence.
      this.envelope[i]=Math.max(value,this.envelope[i]*0.997,0.12);
      const normalized=this.autoNormalize&&value>0.008?value/this.envelope[i]*0.65:value;
      this.targets[i]=Math.min(1,normalized*this.sensitivity);
    }
    this.lastPacket = performance.now();
  }
  draw(now) {
    const dt = Math.min(0.08, Math.max(0, (now - this.lastFrame) / 1000));
    this.lastFrame = now;
    const canvas = this.canvas;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const ctx = this.ctx;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#f4f0e9';
    const gap = 3, barWidth = Math.max(1, (w - (this.count-1) * gap) / this.count);
    const connected = now - this.lastPacket < 300;
    for (let i = 0; i < this.count; i++) {
      const target = connected ? this.targets[i] : 0;
      const response = (target > this.values[i] ? 30 : 12) * Math.pow(4, 0.7 - this.smoothing);
      this.values[i] += (target - this.values[i]) * (1 - Math.exp(-dt * response));
      const barHeight = Math.max(2, this.values[i] * h);
      ctx.globalAlpha = 0.65 + 0.35 * this.values[i];
      const x=i*(barWidth+gap), width=Math.min(barWidth,3);
      ctx.fillRect(x,h-barHeight,width,barHeight);
      if(this.peakHold){
        if(this.values[i]>=this.peaks[i]){this.peaks[i]=this.values[i];this.peakTimes[i]=now}
        else if(now-this.peakTimes[i]>450)this.peaks[i]=Math.max(this.values[i],this.peaks[i]-dt*0.75);
        ctx.globalAlpha=0.9;
        ctx.fillRect(x,h-Math.max(2,this.peaks[i]*h),width,2);
      }
    }
    ctx.globalAlpha = 1;
    requestAnimationFrame(t => this.draw(t));
  }
}
