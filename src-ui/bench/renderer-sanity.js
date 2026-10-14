// @ts-nocheck
// Standalone diagnostic: dynamic fixture/API shapes are checked by capture counts,
// engine errors, and rendered output. It is not imported by the product app.
// Deliberately separate from the app. Frozen geometry comes from Iron's saved
// document, with its real paths, styles, order and field-relative transforms.
const params = new URLSearchParams(location.search);
const count = Number(params.get('shapes') || 1000);
const kind = params.get('renderer') || 'canvas2d';
const zoom = Number(params.get('zoom') || 1);
const fixture = await (await fetch(`.agents/render-perf/fixtures/stress-${count}.json`)).json();
const output = document.querySelector('#result');
const canvas = document.querySelector('#canvas');
const [W,H] = fixture.physicalSize;
canvas.width=W;canvas.height=H;
const cssSize=()=>{canvas.style.width=`${W/devicePixelRatio}px`;canvas.style.height=`${H/devicePixelRatio}px`;};
cssSize();
const multiply = (a,b) => [a[0]*b[0]+a[2]*b[1],a[1]*b[0]+a[3]*b[1],a[0]*b[2]+a[2]*b[3],a[1]*b[2]+a[3]*b[3],a[0]*b[4]+a[2]*b[5]+a[4],a[1]*b[4]+a[3]*b[5]+a[5]];
const matrix=(x,y,r=0)=>[Math.cos(r),Math.sin(r),-Math.sin(r),Math.cos(r),x,y];
const scaled=m=>[m[0]*zoom,m[1]*zoom,m[2]*zoom,m[3]*zoom,m[4]*zoom,m[5]*zoom];
const color=c=>`rgba(${c[0]*255},${c[1]*255},${c[2]*255},${c[3]})`;
function deformedArgs(op,args,f) {
 const a=args.map((v,i)=>v*(i%2?2-f:f));
 // Control-point displacements are not an affine of the original path.
 if(op==='bezierCurveTo'){a[1]+=10*(f-1);a[3]-=10*(f-1);}
 return a;
}
function commands(target, shape, factor=1) {
  if(shape.kind==='rect')target.rect(0,0,shape.w*factor,shape.h*(2-factor));
  else if(shape.kind==='circle')target.arc(0,0,shape.r*factor,0,Math.PI*2);
  else for(const [op,...args] of shape.path)target[op](...deformedArgs(op,args,factor));
}
const shapes=fixture.shapes;
if(fixture.format!==1||shapes.length!==count+1)throw new Error('unexpected fixture format/count');
// Identical motion/geometry inputs to all backends, counted in submission time.
// No precomputed frames, no bitmap cache, no unchanged local geometry in the
// all-geometry case. Each path is resized by a distinct continuous factor.
const factors=new Float64Array(shapes.length).fill(1);
const offsets=new Float64Array(shapes.length*2);
function motion(state) {
 for(let i=0;i<shapes.length;i++) {
  factors[i]=state.geometry?1+.2*Math.sin(state.t*2+i*.618):1;
  offsets[i*2]=state.independent?12*Math.sin(state.t*2+i*.618):0;
  offsets[i*2+1]=state.independent?8*Math.cos(state.t*1.7+i*.414):0;
 }
}
let renderer;
if(kind==='canvas2d') {
 const context=canvas.getContext('2d',{alpha:true});
 const paths=shapes.map(s=>{const p=new Path2D();commands(p,s);return p;});
 renderer={version:'browser built-in',draw(state){
  context.setTransform(1,0,0,1,0,0);context.clearRect(0,0,W,H);
  context.fillStyle='#f6f7f8';context.fillRect(0,0,W,H);
  context.lineJoin='miter';context.miterLimit=4;
  const root=matrix(fixture.field[0]+state.dx,fixture.field[1]+state.dy,state.angle);
  for(let i=0;i<shapes.length;i++) {
   const s=shapes[i];const local=s.matrix.slice();local[4]+=offsets[i*2];local[5]+=offsets[i*2+1];let m=multiply(root,local);
   const p=state.geometry?new Path2D():paths[i];
   if(state.geometry)commands(p,s,factors[i]);
   if(i===fixture.sampleIndex){m[4]+=state.leafDx;m[5]+=state.leafDy;}
   context.setTransform(...scaled(m));
   if(s.fill){context.fillStyle=color(s.fill);context.fill(p,'nonzero');}
   if(s.stroke){context.strokeStyle=color(s.stroke.color);context.lineWidth=s.stroke.width;context.stroke(p);}
   if(state.highlight){context.strokeStyle='rgba(26,102,255,0.5019607843137255)';context.lineWidth=2;context.stroke(p);}
  }
 }};
} else if(kind.startsWith('vello-')) {
 const mod=await import(/* @vite-ignore */ '/Ironfell/.agents/render-perf/vello-probe/iron_renderer_probe.js');
 await mod.default();
 const probe=await mod.create_probe(canvas,JSON.stringify(fixture),zoom,kind==='vello-wg');
 renderer={version:'vello_hybrid 0.2.0 / wgpu 29 / opt3 SIMD',draw(s){probe.draw(s.dx,s.dy,s.angle,s.leafDx,s.leafDy,s.highlight,s.geometry,factors,offsets);}};
} else {
 const mod=await import(/* @vite-ignore */ '/Ironfell/.agents/render-perf/thorvg/package/dist/webcanvas.esm.js');
 const TVG=await mod.default.init({renderer:kind==='thorvg-wg'?'wg':'gl',locateFile:p=>new URL(`.agents/render-perf/thorvg/package/dist/${p}`,location.href).href});
 const target=new TVG.Canvas('#canvas',{width:W,height:H,enableDevicePixelRatio:false,engineOption:TVG.EngineOption.None});
 target.add(new TVG.Shape().appendRect(0,0,W,H).fill(246,247,248,255));
 cssSize();
 const root=new TVG.Scene();const items=[],outlines=[];let previousHighlight;
 const transform=m=>({e11:m[0],e12:m[2],e13:m[4],e21:m[1],e22:m[3],e23:m[5],e31:0,e32:0,e33:1});
 const geometry=(p,s,f=1)=>{
  if(s.kind==='rect')p.appendRect(0,0,s.w*f,s.h*(2-f));
  else if(s.kind==='circle')p.appendCircle(0,0,s.r*f);
  else for(const [op,...args] of s.path) {
   const method={moveTo:'moveTo',lineTo:'lineTo',bezierCurveTo:'cubicTo',closePath:'close'}[op];p[method](...deformedArgs(op,args,f));
  }
  return p;
 };
 const paint=s=>{
  const p=geometry(new TVG.Shape(),s);
  p.transform(transform(s.matrix));return p;
 };
 const bytes=c=>c.map(v=>Math.round(v*255));
 for(const s of shapes) {
  const p=paint(s);
  if(s.fill)p.fill(...bytes(s.fill));else p.fill(0,0,0,0);
  if(s.stroke)p.stroke({color:bytes(s.stroke.color),width:s.stroke.width,miterLimit:4});
  root.add(p);items.push(p);
  const h=paint(s).fill(0,0,0,0).stroke({color:[26,102,255,128],width:2,miterLimit:4}).opacity(0);
  root.add(h);outlines.push(h);
 }
 target.add(root);
 let wasGeometry=false;
 renderer={version:TVG.version,draw(state){
  root.transform(transform(scaled(matrix(fixture.field[0]+state.dx,fixture.field[1]+state.dy,state.angle))));
  for(let i=0;i<shapes.length;i++) {
   const s=shapes[i];const m=s.matrix.slice();m[4]+=offsets[i*2];m[5]+=offsets[i*2+1];
   if(i===fixture.sampleIndex){m[4]+=state.leafDx;m[5]+=state.leafDy;}
   // Update all local poses only in independent animation; do not penalize
   // retained renderers with redundant setter calls in the group-only case.
   const moved=state.independent||renderer.wasIndependent||i===fixture.sampleIndex;
   if(moved)items[i].transform(transform(m));
   // Hidden outlines stay in their original local pose. None of the animated
   // leaf/geometry scenarios displays them; avoid redundant WASM setters.
   if(state.highlight&&moved)outlines[i].transform(transform(m));
   if(state.geometry||wasGeometry){geometry(items[i].reset(),s,factors[i]);if(state.highlight)geometry(outlines[i].reset(),s,factors[i]);}
  }
  wasGeometry=state.geometry;renderer.wasIndependent=state.independent;
  if(state.highlight!==previousHighlight){for(const h of outlines)h.opacity(state.highlight?255:0);previousHighlight=state.highlight;}
  target.update().render();
 }};
}
if(canvas.width!==W||canvas.height!==H)throw new Error(`wrong target size: ${canvas.width}x${canvas.height}, expected ${W}x${H}`);
const stateFor=(scenario,t)=>({
 dx:scenario==='group-drag'||scenario==='group-plain'?100*Math.sin(t*2):0,
 dy:scenario==='group-drag'||scenario==='group-plain'?30*Math.sin(t*2):0,
 angle:scenario==='spin'?t*1.5:0,
 leafDx:scenario==='leaf-drag'?100*Math.sin(t*2):0,
 leafDy:scenario==='leaf-drag'?30*Math.sin(t*2):0,
 highlight:scenario==='hover'?Math.floor(t*4)%2===1:scenario==='group-drag',
 t,independent:scenario==='all-transform'||scenario==='all-geometry',geometry:scenario==='all-geometry',
});
const draw=s=>{motion(s);renderer.draw(s);};
draw(stateFor('static',0));
const percentile=(values,p)=>{const v=values.slice().sort((a,b)=>a-b);return Number(v[Math.min(v.length-1,Math.floor(v.length*p))].toFixed(3));};
async function run(scenario,duration=3000,warmup=500) {
 const work=[],ts=[];let start;
 await new Promise((resolve,reject)=>{
  const frame=time=>{try{
   start??=time;const elapsed=time-start;const before=performance.now();draw(stateFor(scenario,elapsed/1000));
   if(elapsed>=warmup){ts.push(time);work.push(performance.now()-before);}
   if(elapsed<warmup+duration)requestAnimationFrame(frame);else resolve();
  }catch(e){reject(e);}};requestAnimationFrame(frame);
 });
 const deltas=ts.slice(1).map((t,i)=>t-ts[i]);
 const result={renderer:kind,version:renderer.version,shapes:shapes.length,scenario,zoom,physical_size:[W,H],dpr:devicePixelRatio,
  frames:ts.length,fps:(ts.length-1)*1000/(ts.at(-1)-ts[0]),raf_delta:{p50:percentile(deltas,.5),p95:percentile(deltas,.95),p99:percentile(deltas,.99),max:Math.max(...deltas)},
  submission_ms:{p50:percentile(work,.5),p95:percentile(work,.95),p99:percentile(work,.99),max:Math.max(...work)},over24ms:deltas.filter(d=>d>24).length};
 output.textContent=JSON.stringify(result,null,2);return result;
}
window.__rendererSanity={run,fixture,kind,ready:true,reset:()=>draw(stateFor('static',0)),snapshot:(scenario,t)=>draw(stateFor(scenario,t))};
output.textContent=`Ready: ${kind}, ${shapes.length} shapes, ${W}×${H} physical pixels, zoom ${zoom}.`;
document.querySelector('#renderer').value=kind;
document.querySelector('#renderer').onchange=e=>{params.set('renderer',e.target.value);location.search=params;};
for(const button of document.querySelectorAll('[data-case]'))button.onclick=()=>run(button.dataset.case).catch(e=>output.textContent=String(e));
