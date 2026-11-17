// Handle acquisition outside bounds, selection-free use and authored resizing at 1x/2x.
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const dpr = Number(process.env.DPR ?? 1);
const out = resolve(import.meta.dir, '../../../.agents/slider'); mkdirSync(out,{recursive:true});
const browser = await chromium.launch({executablePath:process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page = await browser.newPage({viewport:{width:1680,height:1000},deviceScaleFactor:dpr});
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
 // Both starts are on the visible handle, beyond the nominal left/right edge.
 await drag(94,112,250,112);d=await save();assert(Math.abs(d.facets.slider[plain].value-0.5)<0.03,'left overhang did not scrub');assert(!(await selected())?.includes(plain),'using plain slider selected it');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 await drag(306,212,200,212);d=await save();
 const entity=libEntity;assert(Math.abs(d.facets.args[entity].level-10)<0.4,'right overhang did not scrub library slider');assert(!(await selected())?.includes(lib),'using library slider selected it');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // Track selects and moves; undo restores placement.
 await drag(200,112,220,142);d=await save();assert(Math.abs(d.facets.transform2d[plain].x-120)<1,'track did not move slider');assert((await selected())?.includes(plain),'track did not select slider');
 await page.evaluate(()=>(window as any).__iron.undo());await wait();
 // Resize the selected plain slider using its bottom-right box grip.
 await drag(400,124,460,124);d=await save();assert(Math.abs(d.facets.size[plain].w-360)<1,'plain slider width grip failed');
 await call('select',[JSON.stringify([{node:lib,panel:'slider-bench'}]),'replace']);await wait();
 await drag(300,224,400,224);d=await save();assert(Math.abs(d.facets.size[lib].w-300)<1,'library width grip failed');
 await page.evaluate(({plain,entity})=>(window as any).__iron.edit(`<edit><set id="${plain}" slider.value="0.4"/><set id="${entity}" args.level="8"/></edit>`),{plain,entity});
 await call('select',['[]','replace']);await page.mouse.move(20,20);await wait();
 await page.emulateMedia({colorScheme:'light'});await wait();await page.screenshot({path:resolve(out,`light-${dpr}x.png`)});
 await page.emulateMedia({colorScheme:'dark'});await wait();await page.screenshot({path:resolve(out,`dark-${dpr}x.png`)});
 assert(!errors.length,errors.join('\n'));console.log(`PASS slider DPR ${dpr}: endpoint grabs, no selection on use, track move, plain/library width grips, light/dark; no page errors`);
} finally {await browser.close()}
