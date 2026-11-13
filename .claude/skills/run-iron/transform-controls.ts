// Real control authoring gestures at CSS pixel coordinates; run DPR=1 and DPR=2.
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const dpr = Number(process.env.DPR ?? 1);
const out = resolve(import.meta.dir, '../../../.agents/transform-controls'); mkdirSync(out, {recursive:true});
const browser = await chromium.launch({executablePath:process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page = await browser.newPage({viewport:{width:1680,height:1000},deviceScaleFactor:dpr});
page.setDefaultTimeout(15000);
const errors:string[]=[]; page.on('pageerror',e=>errors.push(String(e))); page.on('console',m=>{if(m.text().includes('panicked at'))errors.push(m.text());});
const assert=(ok:unknown,why:string)=>{if(!ok)throw new Error(why);};
const call=(name:string,args:unknown[]=[])=>page.evaluate(({name,args})=>(window as any).__iron.call(name,args),{name,args});
const save=()=>page.evaluate(async()=>JSON.parse(await (window as any).__iron.save()));
const until=async(test:()=>Promise<boolean>)=>{const end=Date.now()+10000;while(Date.now()<end){if(await test())return;await page.waitForTimeout(50);}throw new Error('state did not settle');};
const wait=()=>page.waitForTimeout(180);
const undo=async()=>{await page.evaluate(()=>(window as any).__iron.undo());await wait();};
const content=(d:any)=>JSON.stringify([d.nodes,d.facets,d.relations]);
try {
  await page.goto(process.env.URL ?? 'http://localhost:5182/Ironfell/?gfx=webgl2&demo&showcase=0&layout=default');
  await page.waitForSelector('.loading',{state:'detached',timeout:60000});
  await until(async()=>{try{return (await page.evaluate(()=>(window as any).__iron.view({fidelity:'full'}))).includes('name="square"');}catch{return false;}});
  const r=await page.evaluate(()=>(window as any).__iron.edit(`<edit>
    <create id="$c" type="composition" name="Pivot bench" notation="free" dims="2d"/>
    <create id="$a" type="rect" parent="$c" name="Needle" x="160" y="200" w="180" h="40" color="#e9aa27"/>
    <create id="$b" type="rect" parent="$c" name="Counterweight" x="120" y="210" w="30" h="20" color="#445577"/>
  </edit>`));assert(r.ok,JSON.stringify(r));
  let d=await save(); const named=(name:string)=>Object.keys(d.facets.name).find(id=>d.facets.name[id].text===name)!;
  const comp=named('Pivot bench'), a=named('Needle'), b=named('Counterweight');
  await page.evaluate(comp=>(window as any).__dock.addPanel({id:'pivot-bench',component:'view',title:'Pivot bench',params:{spec:{source:comp,notation:'composition'}},position:{referencePanel:'viewer',direction:'within'}}),comp);
  await wait();
  const box=await page.locator('[data-panel-id="pivot-bench"]').boundingBox();assert(box,'no view');
  const xy=(x:number,y:number)=>({x:box!.x+x,y:box!.y+y});
  const click=async(x:number,y:number)=>{const p=xy(x,y);await page.mouse.move(p.x,p.y);await wait();await page.mouse.down();await wait();await page.mouse.up();await wait();};
  const drag=async(x:number,y:number,u:number,v:number)=>{const p=xy(x,y),q=xy(u,v);await page.mouse.move(p.x,p.y);await wait();await page.mouse.down();await wait();await page.mouse.move(q.x,q.y,{steps:12});await wait();await page.mouse.up();await wait();};
  const select=async(ids:string[])=>{const r=await call('select',[JSON.stringify(ids.map(node=>({node,panel:'pivot-bench'}))),'replace']);assert(r.ok,JSON.stringify(r));await until(async()=>await page.locator('.inspector').getAttribute('data-selection')===ids.join(' '));await wait();};
  const create=()=>page.getByRole('button',{name:'Create control',exact:true}).click();
  await select([a,b]);
  let snapshot=content(await save());
  await create();await page.getByRole('button',{name:'Cancel',exact:true}).click();await wait();
  assert(content(await save())===snapshot,'cancel created a document mutation');
  await create();await page.keyboard.press('Escape');await wait();assert(content(await save())===snapshot,'Escape mutated document');
  await create();await click(160,220);
  await until(async()=>Object.values((await save()).nodes).some((n:any)=>n.ty==='empty'&&n.parent===comp));
  d=await save();const control=Object.keys(d.nodes).find(id=>d.nodes[id].ty==='empty'&&d.nodes[id].parent===comp)!;
  assert(Math.abs(d.facets.transform2d[control].x-160)<0.1 && Math.abs(d.facets.transform2d[control].y-220)<0.1,'chosen pivot misplaced');
  assert(d.nodes[a].parent===control && d.facets.transform2d[a].x===0 && d.facets.transform2d[a].y===-20,'creation moved artwork');
  console.log('creation: chosen pivot, cancellation, siblings, artwork preserved');
  await select([control]);
  snapshot=content(await save());
  await page.getByRole('button',{name:'Place pivot',exact:true}).click();await click(250,260);
  await until(async()=>Math.abs((await save()).facets.transform2d[control].x-250)<0.1);
  d=await save();assert(d.facets.transform2d[a].x===-90 && d.facets.transform2d[a].y===-60,'pivot relocation moved artwork');
  await undo();assert(content(await save())===snapshot,'pivot undo not atomic');
  await page.getByRole('button',{name:'Place pivot',exact:true}).click();await page.getByRole('button',{name:'Use selection centre',exact:true}).click();
  await until(async()=>Math.abs((await save()).facets.transform2d[control].x-230)<0.1);
  await undo();
  // Tag is 16 CSS px right of the origin; dragging it moves the contents.
  await drag(185,207,225,237);
  await until(async()=>Math.abs((await save()).facets.transform2d[control].x-200)<0.2);
  d=await save();assert(Math.abs(d.facets.transform2d[control].y-250)<0.2,'tag drag did not move contents');
  await undo();
  // The rotation handle is 34 CSS px above the origin, even at 2x DPR.
  await drag(160,186,194,220);
  await until(async()=>Math.abs((await save()).facets.transform2d[control].rot)>1.4);
  d=await save();assert(Math.abs(d.facets.transform2d[control].x-160)<0.1 && Math.abs(d.facets.transform2d[control].y-220)<0.1,'rotation moved actual origin');
  await undo();
  const scaled=await call('inspect_patch',[control,JSON.stringify({values:{'transform2d.sx':'3','transform2d.sy':'0.5'},label:'scale control'})]);assert(scaled.ok,JSON.stringify(scaled));await wait();
  await drag(160,186,194,220);
  await until(async()=>Math.abs((await save()).facets.transform2d[control].rot)>1.4);
  d=await save();assert(Math.abs(d.facets.transform2d[control].x-160)<0.1,'scaled control handle shifted origin');
  await undo();await undo();
  console.log('pivot: pose preserved, centre shortcut, atomic undo; tag moves contents; rotation fixes origin');
  await page.getByRole('checkbox',{name:'Show controls',exact:true}).click();
  assert(await page.getByRole('checkbox',{name:'Show controls',exact:true}).getAttribute('data-state')==='unchecked','visibility did not synchronize');
  await page.getByRole('checkbox',{name:'Show controls',exact:true}).click();
  await page.screenshot({path:resolve(out,`controls-${dpr}x.png`)});
  await page.emulateMedia({colorScheme:'dark'});await page.screenshot({path:resolve(out,`controls-dark-${dpr}x.png`)});
  // Snapping uses the same target features as ordinary moves, plus path anchors.
  const added=await page.evaluate(comp=>(window as any).__iron.edit(`<edit>
    <create id="$r" type="rect" parent="${comp}" name="Snap target" x="400" y="300" w="100" h="60" color="#bbbbbb"/>
    <create id="$p" type="path" parent="${comp}" name="Snap path" x="400" y="500"/>
    <create id="$c" type="contour" parent="$p"/>
    <create id="$a" type="point" parent="$c" x="0" y="0"/>
    <create id="$b" type="point" parent="$c" x="80" y="20"/>
  </edit>`),comp);assert(added.ok,JSON.stringify(added));
  await wait();await select([control]);
  await page.getByRole('button',{name:'Place pivot',exact:true}).click();await click(449,329);
  await until(async()=>{const t=(await save()).facets.transform2d[control];return t.x===450&&t.y===330;});await undo();
  await page.getByRole('button',{name:'Place pivot',exact:true}).click();await click(479,519);
  await until(async()=>{const t=(await save()).facets.transform2d[control];return t.x===480&&t.y===520;});await undo();
  // Selection changes and undo cancel the temporary mode without a placement.
  await page.getByRole('button',{name:'Place pivot',exact:true}).click();await select([a]);
  await until(async()=>await page.getByRole('button',{name:'Use selection centre',exact:true}).count()===0);
  await select([control]);
  const bind=await page.evaluate(control=>(window as any).__iron.edit(`<edit><set id="${control}" rot="{= 0.5}"/></edit>`),control);assert(bind.ok,JSON.stringify(bind));await wait();
  snapshot=content(await save());await page.getByRole('button',{name:'Place pivot',exact:true}).click();
  await until(async()=>(await page.locator('.inspector .error').textContent()??'').includes('static'));
  assert(content(await save())===snapshot,'bound pivot refusal changed artwork');await undo();
  console.log('snapping to centres/path anchors, selection cancellation and bound-control refusal: pass');
  await select([a,b]);await create();await page.getByRole('button',{name:'Use selection centre',exact:true}).click();
  await until(async()=>Object.values((await save()).nodes).filter((n:any)=>n.ty==='empty'&&!n.tombstoned&&n.parent===control).length===1);
  console.log('centre creation and visibility: pass');
  assert(errors.length===0,errors.join('\n'));console.log(`transform-controls DPR=${dpr}: pass (no page errors)`);
} catch(e) {await page.screenshot({path:resolve(out,`failure-${dpr}x.png`)});throw e;} finally {await browser.close();}
