// PipeWire capture arrives as mono signed 16-bit PCM. The browser performs
// the FFT with Web Audio and renders its own six-band visualisation.
export class Spectrum {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.values = new Float32Array(6);
    this.lastFrame = performance.now();
    this.lastPacket = 0;
    this.context = null;
    this.analyser = null;
    this.nextTime = 0;
    this.frequency = null;
    this.pcmBands = new Float32Array(6);
    this.pcmHistory = Array.from({length:6},()=>new Float32Array(5));
    this.historyIndex = 0;
    this.sampleWindow = new Float32Array(1024);
    this.windowCursor = 0;
    requestAnimationFrame(t => this.draw(t));
  }
  init() {
    if (this.context) return;
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) return;
    this.context = new AudioContextClass();
    this.analyser = this.context.createAnalyser();
    this.analyser.fftSize = 1024;
    this.analyser.smoothingTimeConstant = 0.68;
    this.analyser.minDecibels = -95;
    this.analyser.maxDecibels = -22;
    this.frequency = new Uint8Array(this.analyser.frequencyBinCount);
    // Keep the graph processing but never play captured audio back through speakers.
    const mute = this.context.createGain();
    mute.gain.value = 0;
    this.analyser.connect(mute);
    mute.connect(this.context.destination);
  }
  receive(packet) {
    if (!(packet instanceof ArrayBuffer) || packet.byteLength < 2) return;
    this.init();
    if (!this.context) return;
    if (this.context.state === 'suspended') this.context.resume().catch(() => {});
    const view = new DataView(packet);
    const samples = packet.byteLength / 2;
    const buffer = this.context.createBuffer(1, samples, 22050);
    const channel = buffer.getChannelData(0);
    for (let i = 0; i < samples; i++) {
      const value = view.getInt16(i * 2, true) / 32768;
      channel[i] = value;
      this.sampleWindow[this.windowCursor++ % 1024] = value;
    }
    this.analysePCM();
    const now = this.context.currentTime;
    // A small jitter buffer keeps Web Audio fed continuously despite WebSocket timing.
    if (this.nextTime < now + .055 || this.nextTime > now + .35)
      this.nextTime = now + .085;
    const source = this.context.createBufferSource();
    source.buffer = buffer;
    source.connect(this.analyser);
    source.start(this.nextTime);
    this.nextTime += buffer.duration;
    source.onended = () => source.disconnect();
    this.lastPacket = performance.now();
  }
  analysePCM() {
    // Browser-side FFT fallback works even when Firefox suspends Web Audio autoplay.
    // No fake animation or time-driven random levels: every bar is derived from PCM.
    const n=1024, real=new Float32Array(n), imag=new Float32Array(n);
    for(let i=0;i<n;i++){
      real[i]=this.sampleWindow[(this.windowCursor+i)%n]*(.5-.5*Math.cos(2*Math.PI*i/(n-1)));
    }
    for(let i=1,j=0;i<n;i++){
      let bit=n>>1;
      for(;j&bit;bit>>=1)j^=bit;
      j^=bit;
      if(i<j){const t=real[i];real[i]=real[j];real[j]=t}
    }
    for(let len=2;len<=n;len<<=1){
      const angle=-2*Math.PI/len;
      for(let base=0;base<n;base+=len){
        for(let j=0;j<len/2;j++){
          const c=Math.cos(angle*j),q=Math.sin(angle*j);
          const x=base+j,y=x+len/2;
          const tr=real[y]*c-imag[y]*q,ti=real[y]*q+imag[y]*c;
          real[y]=real[x]-tr;imag[y]=imag[x]-ti;
          real[x]+=tr;imag[x]+=ti;
        }
      }
    }
    const edges=[55,150,350,800,1800,4000,9000];
    for(let b=0;b<6;b++){
      const lo=Math.max(1,Math.floor(edges[b]*n/22050));
      const hi=Math.min(n/2,Math.ceil(edges[b+1]*n/22050));
      let sum=0,count=0,peak=0;
      for(let k=lo;k<hi;k++){
        const magnitude=Math.hypot(real[k],imag[k]);
        sum+=magnitude*magnitude;peak=Math.max(peak,magnitude);count++;
      }
      const rms=count?Math.sqrt(sum/count):0;
      // Preserve short transients and expand the small amplitudes seen on PipeWire.
      const level=Math.min(1,Math.pow((rms*.65+peak*.35)/.85,.55));
      this.pcmHistory[b][this.historyIndex%5]=level;
      this.pcmBands[b]=Math.max(...this.pcmHistory[b]);
    }
    this.historyIndex++;
  }
  draw(now) {
    const dt = Math.min(.08, (now - this.lastFrame) / 1000);
    this.lastFrame = now;
    const canvas = this.canvas, w = canvas.clientWidth, h = canvas.clientHeight;
    const dpr = Math.min(devicePixelRatio || 1, 2);
    if (canvas.width !== Math.round(w*dpr) || canvas.height !== Math.round(h*dpr)) {
      canvas.width = Math.round(w*dpr); canvas.height = Math.round(h*dpr);
    }
    const ctx = this.ctx;
    ctx.setTransform(dpr,0,0,dpr,0,0);
    ctx.clearRect(0,0,w,h);
    if (this.analyser && this.frequency) this.analyser.getByteFrequencyData(this.frequency);
    const live = now - this.lastPacket < 500;
    const gap = 3, width = Math.max(1, (w - 5*gap)/6);
    ctx.fillStyle = '#f4f0e9';
    for (let i = 0; i < 6; i++) {
      // Logarithmic bands: bass, low-mid, mid, upper-mid, presence, treble.
      const edges = [55, 150, 350, 800, 1800, 4000, 9000];
      const rate = this.context?.sampleRate || 22050;
      const low = Math.max(1, Math.floor(edges[i] * 1024 / rate));
      const high = Math.min(this.frequency?.length || 0, Math.ceil(edges[i+1] * 1024 / rate));
      let energy = 0, count = 0;
      if (live && this.frequency) {
        for (let j = low; j < high; j++) {
          energy += this.frequency[j] / 255;
          count++;
        }
      }
      const level = count ? energy / count : 0;
      const webAudioLevel = Math.min(1, Math.pow(Math.max(0, (level - .025) * 2.3), .8));
      // The direct PCM FFT remains responsive even if Firefox suspends its audio graph.
      const target = live ? Math.max(webAudioLevel, this.pcmBands[i]) : 0;
      this.values[i] += (target - this.values[i]) * (1 - Math.exp(-dt * (target > this.values[i] ? 23 : 9)));
      const height = Math.max(2, this.values[i] * h);
      ctx.globalAlpha = .65 + .35 * this.values[i];
      ctx.fillRect(i * (width + gap), h - height, Math.min(width,3), height);
    }
    ctx.globalAlpha = 1;
    requestAnimationFrame(t => this.draw(t));
  }
}
