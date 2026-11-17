// Handle acquisition outside bounds, selection-free use and authored resizing at 1x/2x.
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
const dpr = Number(process.env.DPR ?? 1);
const out = resolve(import.meta.dir, '../../../.agents/slider'); mkdirSync(out,{recursive:true});
const browser = await chromium.launch({executablePath:process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page = await browser.newPage({viewport:{width:1680,height:1000},deviceScaleFactor:dpr,colorScheme:'dark'});
const errors:string[]=[];page.on('pageerror',e=>errors.push(String(e)));
const assert=(ok:unknown,why:string)=>{if(!ok)throw new Error(why)};
const wait=()=>page.waitForTimeout(220);
const save=()=>page.evaluate(async()=>JSON.parse(await (window as any).__iron.save()));
const call=(name:string,args:unknown[]=[])=>page.evaluate(({name,args})=>(window as any).__iron.call(name,args),{name,args});
try {
 await page.goto(process.env.URL ?? 'http://localhost:5173/Ironfell/?gfx=webgl2&demo&showcase=0&layout=default');
 await page.waitForSelector('.loading',{state:'detached',timeout:60000});
 await page.waitForFunction(async()=>{try{return (await (window as any).__iron.view({fidelity:'full'})).includes('name="square"')}catch{return false}},undefined,{timeout:60000});
 await page.waitForFunction(()=>(globalThis as any).__documentOpened!==undefined,undefined,{timeout:60000}); await wait();
 const r=await page.evaluate(()=>(window as any).__iron.edit(`<edit>
 <composition id="$c" name="Slider bench" notation="free" dims="2d" grid_snap="false"/>
 <slider in="$c" name="Plain" x="100" y="100" size.w="300" size.h="24" slider.value="0"/>
 <place in="$c" def="std:slider" name="Library" x="100" y="200" level="20"/>
 </edit>`));assert(r.ok,JSON.stringify(r));
 let d=await save();const named=(name:string)=>Object.keys(d.facets.name).find(id=>d.facets.name[id].text===name)!;
 const comp=named('Slider bench'),plain=named('Plain'),libEntity=named('Library'),lib=(d.relations as any[]).find(r=>r.to===libEntity&&r.rel==='figures').from;
 await page.evaluate(comp=>(window as any).__dock.addPanel({id:'slider-bench',component:'view',title:'Slider bench',params:{spec:{source:comp,notation:'composition'}},position:{referencePanel:'viewer',direction:'within'}}),comp);await wait();
 const box=(await page.locator('[data-panel-id="slider-bench"]').boundingBox())!;assert(box,'no view');
 const drag=async(x:number,y:number,u:number,v:number)=>{await page.mouse.move(box.x+x,box.y+y);await wait();await page.mouse.down();await wait();await page.mouse.move(box.x+u,box.y+v,{steps:12});await wait();await page.mouse.up();await wait()};
 const selected=async()=>await page.locator('.inspector').getAttribute('data-selection');
 const crop={x:box.x+80,y:box.y+80,width:340,height:160};
 for (const [name,x,y] of [['plain',200,112],['library',200,212]] as const) {
   await page.mouse.move(box.x+x,box.y+y);await wait();
   await page.screenshot({path:resolve(out,`hover-${name}-${dpr}x.png`),clip:crop});
 }
 execFileSync('python3',['-c',`from PIL import Image
import sys
for path in sys.argv[2:]:
 im=Image.open(path).convert('RGB')
 blue=sum(1 for r,g,b in im.getdata() if b>100 and b>r*1.3 and b>g*1.1)
 assert blue==0, (path, 'editor hover box', blue)
 ratio=int(sys.argv[1]); x,y=(20,24) if 'plain' in path else (220,124)
 r,g,b=im.getpixel((x*ratio,y*ratio))
 assert r-g>10 and g-b>10, (path,'missing accent hover',r,g,b)
 `,String(dpr),resolve(out,`hover-plain-${dpr}x.png`),resolve(out,`hover-library-${dpr}x.png`)]);

 // Both starts are on the visible handle, beyond the nominal left/right edge.
 await drag(94,112,250,112);d=await save();assert(Math.abs(d.facets.slider[plain].value-0.5)<0.03,'left overhang did not scrub');assert(!(await selected())?.includes(plain),'using plain slider selected it');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 await drag(306,212,200,212);d=await save();
 const entity=libEntity;assert(Math.abs(d.facets.args[entity].level-10)<0.4,'right overhang did not scrub library slider');assert(!(await selected())?.includes(lib),'using library slider selected it');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // A track click changes value without selection or placement changes.
 const click=async(x:number,y:number)=>{await page.mouse.move(box.x+x,box.y+y);await wait();await page.mouse.down();await page.waitForTimeout(30);await page.mouse.up();await wait();};
 await click(325,112);d=await save();assert(Math.abs(d.facets.slider[plain].value-0.75)<0.02,'track click did not set value');assert(!(await selected())?.includes(plain),'track click selected slider');assert(d.facets.transform2d[plain].x===100,'track click moved slider');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();d=await save();assert(d.facets.slider[plain].value===0,'track click did not undo in one step');
 // Clicking then dragging a track switches to direct scrubbing.
 await drag(175,112,280,112);d=await save();assert(Math.abs(d.facets.slider[plain].value-0.6)<0.02,'track drag did not scrub');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // Slow the declared presentation transition for reliable pixel sampling.
 const motion=await page.evaluate(plain=>(window as any).__iron.edit(`<edit><set id="${plain}" affords.style="motion.value: out 800"/></edit>`),plain);assert(motion.ok,JSON.stringify(motion));await wait();
 await page.mouse.move(box.x+325,box.y+112);await wait();await page.mouse.down();await page.waitForTimeout(25);await page.mouse.up();
 const animationClip={x:box.x+90,y:box.y+96,width:320,height:32};
 await page.screenshot({path:resolve(out,`animation-mid-${dpr}x.png`),clip:animationClip});
 d=await save();assert(Math.abs(d.facets.slider[plain].value-0.75)<0.02,'semantic target was delayed by animation');
 await page.waitForTimeout(850);await page.screenshot({path:resolve(out,`animation-end-${dpr}x.png`),clip:animationClip});
 execFileSync('python3',['-c',`from PIL import Image
import sys
ratio=int(sys.argv[1])
def center(path):
 im=Image.open(path).convert('RGB'); y=16*ratio
 xs=[x for x in range(im.width) if min(im.getpixel((x,y)))>230]
 assert xs, (path, 'missing thumb')
 return sum(xs)/len(xs)/ratio
mid,end=map(center,sys.argv[2:])
assert 10<mid<end-1, (mid,end,'thumb did not animate')
assert abs(end-235)<2, (end,'wrong final thumb position')
 `,String(dpr),resolve(out,`animation-mid-${dpr}x.png`),resolve(out,`animation-end-${dpr}x.png`)]);
 const reset=await page.evaluate(plain=>(window as any).__iron.edit(`<edit><set id="${plain}" slider.value="0" affords.style=""/></edit>`),plain);assert(reset.ok,JSON.stringify(reset));await wait();
 // The Library instance's cached drawing must animate too.
 d=await save();const definition=d.facets.instance[entity].def.ref;
 const template=Object.keys(d.nodes).find(id=>d.nodes[id].ty==='template'&&d.nodes[id].parent===definition)!;
 const part=Object.keys(d.nodes).find(id=>d.nodes[id].ty==='slider'&&d.nodes[id].parent===template)!;
 const derived=lib+'/'+part.slice(1);
 const libraryMotion=await page.evaluate(derived=>(window as any).__iron.edit(`<edit><set id="${derived}" affords.style="motion.value: out 800"/></edit>`),derived);assert(libraryMotion.ok,JSON.stringify(libraryMotion));await wait();
 await page.mouse.move(box.x+200,box.y+212);await wait();await page.mouse.down();await page.waitForTimeout(25);await page.mouse.up();
 const libraryClip={x:box.x+90,y:box.y+196,width:220,height:32};
 await page.screenshot({path:resolve(out,`library-animation-mid-${dpr}x.png`),clip:libraryClip});
 d=await save();assert(Math.abs(d.facets.args[entity].level-10)<0.1,'Library semantic value delayed');
 await page.waitForTimeout(850);await page.screenshot({path:resolve(out,`library-animation-end-${dpr}x.png`),clip:libraryClip});
 execFileSync('python3',['-c',`from PIL import Image
import sys
ratio=int(sys.argv[1])
def center(path):
 im=Image.open(path).convert('RGB'); xs=[x for x in range(im.width) if min(im.getpixel((x,16*ratio)))>230]
 assert xs, (path,'missing Library thumb')
 return sum(xs)/len(xs)/ratio
mid,end=map(center,sys.argv[2:])
assert end+1<mid<210, (mid,end,'Library drawing did not animate')
assert abs(end-110)<2, (end,'wrong Library final position')
 `,String(dpr),resolve(out,`library-animation-mid-${dpr}x.png`),resolve(out,`library-animation-end-${dpr}x.png`)]);
 const libraryReset=await page.evaluate(({derived,entity})=>(window as any).__iron.edit(`<edit><set id="${derived}" affords.style=""/><set id="${entity}" args.level="20"/></edit>`),{derived,entity});assert(libraryReset.ok,JSON.stringify(libraryReset));await wait();
 // Mod-click selects the whole Library component; it must not change its value.
 await page.keyboard.down('Meta');await click(200,212);await page.keyboard.up('Meta');
 assert((await selected())?.includes(lib),'Mod-click did not select Library component');
 d=await save();assert(d.facets.args[entity].level===20,'Mod-click changed Library value');
 // Holding the selection modifier while dragging moves the component.
 await page.keyboard.down('Meta');await drag(200,212,220,242);await page.keyboard.up('Meta');
 d=await save();assert(Math.abs(d.facets.transform2d[lib].x-120)<1 && Math.abs(d.facets.transform2d[lib].y-230)<1,'modifier drag did not move component');assert(d.facets.args[entity].level===20,'modifier drag changed value');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // Layers/API selection offers the same authoring handles.
 await call('select',[JSON.stringify([{node:plain,panel:'slider-bench'}]),'replace']);await wait();
 // A selected input remains usable away from its selection grips.
 await click(325,112);d=await save();assert(Math.abs(d.facets.slider[plain].value-0.75)<0.02,'selected input did not operate');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // Resize the selected plain slider using its bottom-right box grip.
 await drag(400,124,460,124);d=await save();assert(Math.abs(d.facets.size[plain].w-360)<1,'plain slider width grip failed');
 await call('select',[JSON.stringify([{node:lib,panel:'slider-bench'}]),'replace']);await wait();
 await drag(300,224,400,224);d=await save();assert(Math.abs(d.facets.size[lib].w-300)<1,'library width grip failed');
 await page.evaluate(({plain,entity})=>(window as any).__iron.edit(`<edit><set id="${plain}" slider.value="0.4"/><set id="${entity}" args.level="8"/></edit>`),{plain,entity});
 await call('select',['[]','replace']);await page.mouse.move(20,20);await wait();
 await page.emulateMedia({colorScheme:'light'});await wait();await page.screenshot({path:resolve(out,`light-${dpr}x.png`)});
 await page.emulateMedia({colorScheme:'dark'});await wait();await page.screenshot({path:resolve(out,`dark-${dpr}x.png`)});
 assert(!errors.length,errors.join('\n'));console.log(`PASS slider DPR ${dpr}: endpoint grabs, hover pixels, track click/drag and easing, modifier selection, plain/library width grips, light/dark; no page errors`);
} finally {await browser.close()}
