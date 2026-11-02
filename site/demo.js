import {DiscDirector} from './motion.js';
import {Spectrum} from './spectrum.js';

// This is the exact production HTML, CSS and display renderer; only its data source is mocked.
const root=document.documentElement;
root.dataset.discAppearance='vinyl-cover';
root.dataset.panelPosition='bottom-right';
root.dataset.progressBar='off';
root.dataset.timeDisplay='off';
root.dataset.holeColor='transparent';
root.style.setProperty('--disc-size','65dvh');
root.style.setProperty('--center-hole-radius','.5%');
root.style.setProperty('--center-hole-diameter','1%');
root.style.setProperty('--background-dim','.60');
root.style.setProperty('--background-blur','50px');
root.style.setProperty('--metadata-opacity','.58');
root.style.setProperty('--metadata-font-size','1');
root.style.setProperty('--metadata-scale','1');
document.getElementById('progress').hidden=true;
document.getElementById('track-times').hidden=true;
const director=new DiscDirector({
 discs:[document.getElementById('disc-a'),document.getElementById('disc-b')],
 backdrops:[document.getElementById('backdrop-a'),document.getElementById('backdrop-b')],
 info:document.getElementById('info'),
 settings:{discRotationSeconds:12,reducedMotion:window.matchMedia('(prefers-reduced-motion: reduce)').matches}
});
const spectrum=new Spectrum(document.getElementById('visualizer'),1,.7);
// Paint the first frame synchronously: no asynchronous preloading or track transition.
const cover='./kind-of-blue.jpg';
const disc=document.getElementById('disc-b');
disc.querySelector('.disc-label').style.backgroundImage=`url("${cover}")`;
disc.querySelector('.record-sleeve').style.backgroundImage=`url("${cover}")`;
disc.querySelector('.record-track').style.transform='translateX(50%)';
disc.classList.add('active');
const backdrop=document.getElementById('backdrop-b');
backdrop.style.backgroundImage=`url("${cover}")`;
backdrop.style.opacity='1';
backdrop.classList.add('active');
director.started=true;
director.current=1;
document.getElementById('track').textContent='So What';
document.getElementById('artist').textContent='Miles Davis';
director.setPlaying(!window.matchMedia('(prefers-reduced-motion: reduce)').matches);
const reduce=window.matchMedia('(prefers-reduced-motion: reduce)');
reduce.addEventListener('change',event=>director.setPlaying(!event.matches));
let last=0;
function visualizer(now){
 if(!reduce.matches && now-last>45){
  const t=now/1000;
  spectrum.receive({bands:Array.from({length:6},(_,i)=>Math.min(.85,Math.max(.09,.25+.18*Math.sin(t*2.2+i*1.1)+.13*Math.sin(t*3.6-i*.8))))});
  last=now;
 }
 requestAnimationFrame(visualizer);
}
requestAnimationFrame(visualizer);
