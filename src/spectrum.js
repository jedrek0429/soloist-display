// Rendering only. Frequency analysis and smoothing happen in the Rust bridge.
export class Spectrum {
  constructor(canvas, sensitivity = 1) {
    this.canvas = canvas;
    const gain = Number(sensitivity);
    this.sensitivity = Number.isFinite(gain) && gain >= 0 ? Math.min(gain, 10) : 1;
    this.ctx = canvas.getContext('2d');
    this.targets = new Float32Array(6);
    this.values = new Float32Array(6);
    this.lastPacket = 0;
    this.lastFrame = performance.now();
    requestAnimationFrame(t => this.draw(t));
  }
  receive(packet) {
    if (!packet || !Array.isArray(packet.bands) || packet.bands.length !== 6) return;
    for (let i = 0; i < 6; i++) {
      const value = Number(packet.bands[i]);
      this.targets[i] = Number.isFinite(value) ? Math.max(0, Math.min(1, value * this.sensitivity)) : 0;
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
    const gap = 3, barWidth = Math.max(1, (w - 5 * gap) / 6);
    const connected = now - this.lastPacket < 300;
    for (let i = 0; i < 6; i++) {
      const target = connected ? this.targets[i] : 0;
      const response = target > this.values[i] ? 30 : 12;
      this.values[i] += (target - this.values[i]) * (1 - Math.exp(-dt * response));
      const barHeight = Math.max(2, this.values[i] * h);
      ctx.globalAlpha = 0.65 + 0.35 * this.values[i];
      ctx.fillRect(i * (barWidth + gap), h - barHeight, Math.min(barWidth, 3), barHeight);
    }
    ctx.globalAlpha = 1;
    requestAnimationFrame(t => this.draw(t));
  }
}
