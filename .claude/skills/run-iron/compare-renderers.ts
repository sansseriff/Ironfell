// Capture Iron's actual Stress field and replay it with independent renderers.
// Vite must run on 5173. Run sequentially, without other browser benchmarks.
//   DPR=2 COUNTS=1000 bun run compare-renderers.ts capture
//   DPR=2 COUNTS=1000 bun run compare-renderers.ts run > results.jsonl
// Artifacts/vendor files live under the repo's gitignored .agents directory.
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
import { resolve } from 'node:path';
const root=resolve(import.meta.dir,'../../..');
const artifacts=resolve(root,'.agents/render-perf');
const dpr=Number(process.env.DPR||2);
const counts=(process.env.COUNTS||'1000').split(',').map(Number);
const mode=process.argv[2]||'run';
const browser=await chromium.launch({executablePath:process.env.CHROME||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist']});
const cdp=await browser.newBrowserCDPSession();const system=await cdp.send('SystemInfo.getInfo');
const environment={browser:browser.version(),viewport:[1600,1000],dpr,gpu:system.gpu.devices,gl_renderer:system.gpu.auxAttributes?.glRenderer,feature_status:system.gpu.featureStatus};
const errors:string[]=[];
let failures=0;
const page=await browser.newPage({viewport:{width:1600,height:1000},deviceScaleFactor:dpr});
page.on('pageerror',e=>errors.push(String(e)));
page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
try {
if(mode==='capture') {
 mkdirSync(resolve(artifacts,'fixtures'),{recursive:true});
 for(const count of counts) {
  await page.goto(`http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=${count}`);
  await page.waitForSelector('.loading',{state:'detached',timeout:180000});
  await page.waitForFunction(()=>!!(window as any).__iron);
  await page.waitForTimeout(500);
  let doc:any;
  for(let i=0;i<30;i++) {
   doc=JSON.parse(await page.evaluate(()=>Promise.race([(window as any).__iron.save(),new Promise((_,reject)=>setTimeout(()=>reject(new Error('save request timed out')),10000))])));
   if(Object.values(doc.components.name||{}).some((n:any)=>n.text==='field'))break;
   await page.waitForTimeout(500);
  }
  const names=doc.components.name;
  const field=Object.keys(names).find(id=>names[id].text==='field');
  const sample=Object.keys(names).find(id=>names[id].text==='sample');
  if(!field||!sample)throw new Error('stress fixture did not appear');
  const c=doc.components;
  const children=(id:string)=>Object.keys(doc.nodes).filter(n=>doc.nodes[n].parent===id).sort((a,b)=>doc.nodes[a].order<doc.nodes[b].order?-1:doc.nodes[a].order>doc.nodes[b].order?1:0);
  const trs=(id:string)=>{const t=c.transform2d[id]||{},r=typeof t.rot==='number'?t.rot:0,co=Math.cos(r),si=Math.sin(r);return [co*(t.sx??1),si*(t.sx??1),-si*(t.sy??1),co*(t.sy??1),t.x??0,t.y??0];};
  const mul=(a:number[],b:number[])=>[a[0]*b[0]+a[2]*b[1],a[1]*b[0]+a[3]*b[1],a[0]*b[2]+a[2]*b[3],a[1]*b[2]+a[3]*b[3],a[0]*b[4]+a[2]*b[5]+a[4],a[1]*b[4]+a[3]*b[5]+a[5]];
  const shapes:any[]=[];
  function walk(id:string,parent:number[]) {
   const matrix=mul(parent,trs(id)),kind=doc.nodes[id].ty;
   if(kind==='group'){for(const child of children(id))walk(child,matrix);return;}
   const fill=c.fill[id],stroke=c.stroke[id];
   const shape:any={id,kind,matrix,fill:fill?fill.color.rgba.map((v:number,i:number)=>i===3?v*(fill.opacity??1):v):null,stroke:stroke?{color:stroke.color.rgba,width:stroke.width}:null};
   if(kind==='rect')Object.assign(shape,c.size[id]);
   else if(kind==='circle')shape.r=c.radius[id].r;
   else if(kind==='path') {
    const points=children(id).map(p=>({t:c.transform2d[p],h:c.handles[p]||{}}));
    shape.path=[['moveTo',points[0].t.x,points[0].t.y]];
    const segment=(a:any,b:any)=>{
     if(!(a.h.ox||a.h.oy||b.h.ix||b.h.iy))shape.path.push(['lineTo',b.t.x,b.t.y]);
     else shape.path.push(['bezierCurveTo',a.t.x+(a.h.ox||0),a.t.y+(a.h.oy||0),b.t.x+(b.h.ix||0),b.t.y+(b.h.iy||0),b.t.x,b.t.y]);
    };
    for(let i=1;i<points.length;i++)segment(points[i-1],points[i]);
    if(c.path[id].closed){segment(points.at(-1),points[0]);shape.path.push(['closePath']);}
   } else throw new Error(`unexpected Stress primitive ${kind}`);
   shapes.push(shape);
  }
  for(const id of children(field))walk(id,[1,0,0,1,0,0]);
  const physicalSize=await page.evaluate(()=>{const r=document.querySelector('[data-panel-id="stress"]')!.getBoundingClientRect();return [Math.round(r.width*devicePixelRatio),Math.round(r.height*devicePixelRatio)];});
  await page.waitForTimeout(1500);
  await page.locator('[data-panel-id="stress"]').screenshot({path:resolve(artifacts,`renderer-iron-${count}-dpr${dpr}.png`)});
  const t=c.transform2d[field];
  const fixture={format:1,source:'src/document_bridge/stress.rs / canonical save',requestedShapes:count,field:[t.x,t.y],sampleIndex:shapes.findIndex(s=>s.id===sample),physicalSize,shapes};
  if(shapes.length!==count+1||fixture.sampleIndex<0)throw new Error('wrong exported primitive count/sample');
  writeFileSync(resolve(artifacts,`fixtures/stress-${count}.json`),JSON.stringify(fixture));
  console.log(JSON.stringify({type:'fixture',count,primitives:shapes.length,physicalSize,sha256:createHash('sha256').update(JSON.stringify(fixture)).digest('hex'),environment}));
 }
} else {
 const vendor=resolve(artifacts,'thorvg');
 if(!existsSync(resolve(vendor,'package/dist/thorvg.wasm'))) {
  mkdirSync(vendor,{recursive:true});
  const response=await fetch('https://registry.npmjs.org/@thorvg/webcanvas/-/webcanvas-1.1.2.tgz');
  if(!response.ok)throw new Error(`ThorVG fetch ${response.status}`);
  const bytes=Buffer.from(await response.arrayBuffer());
  const integrity='Vw9UJMNMJ1b2F4hA8zBY3Q0xgfF/9n5pXQbpJfj6a4K3caqJN4lp6T5JmCxG+fACe5kn+BR/xMl5ejlYOgZG2Q==';
  if(createHash('sha512').update(bytes).digest('base64')!==integrity)throw new Error('ThorVG package checksum mismatch');
  writeFileSync(resolve(vendor,'package.tgz'),bytes);execFileSync('tar',['-xzf',resolve(vendor,'package.tgz'),'-C',vendor]);
 }
 for(const count of counts) for(const renderer of (process.env.RENDERERS||'canvas2d,thorvg-gl,thorvg-wg,vello-gl,vello-wg').split(',')) {
  if(renderer.startsWith('vello-')&&!existsSync(resolve(artifacts,'vello-probe/iron_renderer_probe_bg.wasm')))
   throw new Error('Build the standalone control first: sh .claude/skills/run-iron/build-renderer-probe.sh');
  const fixturePath=resolve(artifacts,`fixtures/stress-${count}.json`);
  if(!existsSync(fixturePath)) {
   const archive=resolve(root,`plans/rendering/benchmarks/stress-${count}-fixture-2026-10-13.json.gz`);
   if(!existsSync(archive))throw new Error('capture the fixture first');
   mkdirSync(resolve(artifacts,'fixtures'),{recursive:true});
   writeFileSync(fixturePath,gunzipSync(readFileSync(archive)));
  }
  const zoom=count>=10000?.37:1;
  errors.length=0;
  await page.goto(`http://localhost:5173/Ironfell/renderer-sanity.html?shapes=${count}&renderer=${renderer}&zoom=${zoom}`);
  try {
   await page.waitForFunction(()=>!!(window as any).__rendererSanity?.ready,{},{timeout:20000});
   for(const scenario of (mode==='preview'?[]:(process.env.SCENARIOS||'group-plain,group-drag,spin,leaf-drag,hover,all-transform,all-geometry').split(','))) {
    const result=await page.evaluate(s=>(window as any).__rendererSanity.run(s),scenario);
    if(errors.length)throw new Error(errors.join('\n'));
    console.log(JSON.stringify({type:'result',...result,environment}));
   }
   await page.evaluate(()=>(window as any).__rendererSanity.reset());
   await page.waitForTimeout(100);
   await page.locator('#canvas').screenshot({path:resolve(artifacts,`renderer-${renderer}-${count}-dpr${dpr}.png`)});
   await page.evaluate(()=>(window as any).__rendererSanity.snapshot('all-geometry',.731));
   await page.waitForTimeout(100);
   await page.locator('#canvas').screenshot({path:resolve(artifacts,`renderer-${renderer}-geometry-${count}-dpr${dpr}.png`)});
  } catch(error) {failures++;console.log(JSON.stringify({type:'failed',count,renderer,error:String(error),page_errors:errors.slice(),environment}));}
 }
}
if(errors.length&&mode==='capture')throw new Error(errors.join('\n'));
} finally { await browser.close(); }
if(failures)process.exitCode=1;
