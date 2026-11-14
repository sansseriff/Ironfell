import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const out=resolve(import.meta.dir,'../../../.agents/attachments');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page=await browser.newPage({viewport:{width:1680,height:1000},deviceScaleFactor:Number(process.env.DPR??1),colorScheme:'light'});
page.setDefaultTimeout(15000);
const errors:string[]=[];page.on('pageerror',e=>errors.push(String(e)));page.on('console',m=>{if(m.text().includes('panicked at'))errors.push(m.text());});
const assert=(ok:unknown,why:string)=>{if(!ok)throw new Error(why);};
const wait=()=>page.waitForTimeout(600);
const call=async(name:string,args:unknown[]=[])=>page.evaluate(async({name,args})=>await (window as any).__iron.call(name,args),{name,args});
const save=()=>page.evaluate(async()=>JSON.parse(await (window as any).__iron.save()));
try {
await page.goto(process.env.URL??'http://localhost:5186/Ironfell/?gfx=webgl2&demo&showcase=0&layout=default');
const deadline=Date.now()+180000;
while(!await page.evaluate(async()=>{try{return (await (window as any).__iron.view({fidelity:'full'})).includes('name="square"');}catch{return false;}})) {assert(Date.now()<deadline,'engine did not become ready');await page.waitForTimeout(100);}
const r=await page.evaluate(()=>(window as any).__iron.edit(`<edit>
<composition id="$c" name="attachment bench" notation="free" dims="2d"/>
<path in="$c" id="$f" name="fiber" x="120" y="100"/><add id="$f" stroke.color="#e9aa27" stroke.width="6"/>
<contour in="$f" id="$fc"/><point in="$fc" id="$a" x="0" y="0"/><add id="$a" handles.ox="100" handles.oy="0"/>
<point in="$fc" id="$b" x="300" y="100"/><add id="$b" handles.ix="-100" handles.iy="0"/>
<path in="$c" id="$s" name="shadow" x="120" y="108"/><add id="$s" stroke.color="#555555" stroke.width="8"/>
<contour in="$s" id="$sc"/><point in="$sc" id="$sa" x="0" y="0"/><add id="$sa" handles.ox="100" handles.oy="0"/>
<point in="$sc" id="$sb" x="300" y="100"/><add id="$sb" handles.ix="-100" handles.iy="0"/>
<empty in="$c" id="$ca" name="endpoint A" x="120" y="100"/>
<empty in="$c" id="$cb" name="endpoint B" x="420" y="200"/>
</edit>`));assert(r.ok,JSON.stringify(r));await wait();
let d=await save();const named=(name:string)=>Object.entries(d.facets.name).find(([id,n]:any)=>n.text===name)![0];
const comp=named('attachment bench'),roots=['fiber','shadow','endpoint A','endpoint B'].map(name=>Object.entries(d.facets.name).find(([id,n]:any)=>n.text===name && d.nodes[id].parent===comp)![0]);
await page.evaluate(comp=>(window as any).__dock.addPanel({id:'attachment-bench',component:'view',title:'Attachment bench',params:{spec:{source:comp,notation:'composition'}},position:{referencePanel:'viewer',direction:'within'}}),comp);
const made=await call('make_component',[JSON.stringify(roots),'fiber and shadow']);assert(made.ok,JSON.stringify(made));await wait();
await page.getByRole('button',{name:'Attach points to control',exact:true}).click();await wait();
const task=page.locator('[data-attachment-task]');
for(const i of [0,1])await task.getByRole('button',{name:'Point 1',exact:true}).nth(i).click();
await task.getByRole('button',{name:'Attach 2 points',exact:true}).click();await wait();
d=await save();assert(Object.keys(d.facets.attachment??{}).length===2,'did not attach both starts');
await task.getByRole('button',{name:'Expose as input port',exact:true}).click();await wait();
await task.getByRole('button',{name:'Endpoint control',exact:true}).click();await page.getByRole('option',{name:'endpoint B',exact:true}).click();
for(const i of [0,1])await task.getByRole('button',{name:'Point 2',exact:true}).nth(i).click();
await task.getByRole('button',{name:'Attach 2 points',exact:true}).click();await wait();
await task.getByRole('button',{name:'Expose as output port',exact:true}).click();await wait();
d=await save();assert(Object.keys(d.facets.attachment??{}).length===4,'did not attach both ends');
let info=await call('author',['attachment_info',JSON.stringify({target:made.picture})]);assert(info.ok,JSON.stringify(info));const b=info.controls.find((c:any)=>c.name==='endpoint B');
const patch=await call('inspect_patch',[b.id,JSON.stringify({values:{'transform2d.y':'260'},label:'move endpoint'})]);assert(patch.ok,JSON.stringify(patch));await wait();
await task.getByRole('button',{name:'Edit offset',exact:true}).first().click();await task.getByRole('textbox',{name:'Attachment Y offset',exact:true}).fill('14');await task.getByRole('button',{name:'Save offset',exact:true}).click();await wait();
await task.getByRole('button',{name:'Detach',exact:true}).first().click();await wait();d=await save();assert(Object.keys(d.facets.attachment??{}).length===3,'detach failed');
await page.evaluate(()=>(window as any).__iron.undo());await wait();d=await save();assert(Object.keys(d.facets.attachment??{}).length===4,'detach undo failed');
await page.screenshot({path:resolve(out,'fiber-task-light.png')});await page.emulateMedia({colorScheme:'dark'});await page.screenshot({path:resolve(out,'fiber-task-dark.png')});
const text=await page.evaluate(()=>(window as any).__iron.save());await page.evaluate(t=>(window as any).__iron.load(t),text);await wait();assert(await page.evaluate(()=>(window as any).__iron.save())===text,'save/load changed document');assert(await task.getByRole('button',{name:'Done',exact:true}).count()===0,'task retained an owner after document replacement');assert(errors.length===0,errors.join('\n'));
console.log('PASS: two paths, two shared controls, ports, movement, offsets, detach/undo, save/reload; no page errors');
} catch(e) {await page.screenshot({path:resolve(out,'failure.png')});console.log(await page.locator('[data-attachment-task]').textContent());throw e;} finally {await browser.close();}
