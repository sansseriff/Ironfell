// Run after build.sh + serve.sh start: bun run appearance.ts (DPR=2 optional).
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const dir=resolve(import.meta.dir,'../../../.agents/appearance');mkdirSync(dir,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROME??'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page=await browser.newPage({viewport:{width:1600,height:1000},colorScheme:'light',deviceScaleFactor:Number(process.env.DPR??1)});
page.setDefaultTimeout(15000);
const errors:string[]=[];page.on('pageerror',e=>{errors.push(String(e));console.log('PAGE ERROR',String(e));});page.on('console',m=>{if(m.text().includes('panicked at'))errors.push(m.text());});
const assert=(ok:unknown,why:string)=>{if(!ok)throw new Error(why);};
const wait=()=>page.waitForTimeout(600);
const doc=()=>page.evaluate(async()=>JSON.parse(await (window as any).__iron.save()));
const view=()=>page.evaluate(()=>(window as any).__iron.view({fidelity:'full'}));
const edit=async(xml:string)=>{const r=await page.evaluate(x=>(window as any).__iron.edit(x),xml);assert(r.ok, JSON.stringify(r));await wait();};
try{
await page.goto('http://localhost:5173/Ironfell/?gfx=webgl2&layout=default');await page.waitForSelector('.loading',{state:'detached',timeout:180000});await page.waitForFunction(async () => !!(window as any).__iron && (await (window as any).__iron.view({fidelity:'full'})).includes('name="controls"'), {timeout:180000});await page.waitForTimeout(1000);
const v=await view(),parent=v.match(/<composition id="([^"]+)" name="controls"/)[1];
await edit(`<edit>
<create id="$g" type="group" parent="${parent}" name="opacity scope" x="400" y="60"/><add id="$g" appearance.opacity="0.5"/>
<create id="$a" type="rect" parent="$g" x="0" y="0" w="100" h="100" color="#ff0000"/>
<create id="$b" type="rect" parent="$g" x="50" y="0" w="100" h="100" color="#0000ff"/>
<create id="$r" type="rect" parent="${parent}" name="gradient sample" x="600" y="60" w="200" h="100"/>
<add id="$r" gradient.kind="linear" gradient.start="#ff0000" gradient.end="#0000ff" gradient.x0="0" gradient.y0="0" gradient.x1="1" gradient.y1="0"/>
<create id="$c" type="group" parent="${parent}" name="clipped scope" x="400" y="260"/>
<add id="$c" group_clip.enabled="true" group_clip.shape="ellipse" group_clip.w="100" group_clip.h="100"/>
<create id="$inside" type="rect" parent="$c" x="-40" y="-40" w="180" h="180" color="#00ff00"/>
<create id="$s" type="rect" parent="${parent}" name="scrub sample" x="600" y="260" w="200" h="100" color="#8866dd"/><add id="$s" appearance.opacity="0.8"/>
</edit>`);
await page.locator('.dv-tab',{hasText:/^\s*Controls\s*$/}).click();await wait();
const p=(await page.locator('[data-panel-id="controls"]').boundingBox())!;assert(p,'panel missing');
const click=async(x:number,y:number)=>{await page.mouse.move(p.x+x/Number(process.env.DPR??1),p.y+y/Number(process.env.DPR??1));await page.waitForTimeout(150);await page.mouse.down();await page.waitForTimeout(200);await page.mouse.up();await wait();};
await page.mouse.move(5,5);await wait();await page.screenshot({path:resolve(dir,'appearance.png')});writeFileSync(resolve(dir,'panel.json'),JSON.stringify({...p,dpr:Number(process.env.DPR??1)}));
execFileSync('python3',[resolve(import.meta.dir,'appearance-pixels.py'),dir],{stdio:'inherit'});
await click(405,265);assert((await page.evaluate(()=>(window as any).__iron.selection())).length===0,'clipped corner picked hidden child');
await click(450,310);assert((await page.evaluate(()=>(window as any).__iron.selection())).length===1,'clip centre did not pick');
await click(650,310);const input=page.locator('[data-leaf="size.w"] input');await input.waitFor();
const d=await doc(),key=Object.keys(d.components.name).find(id=>d.components.name[id].text==='scrub sample')!;
const begin=async(delta:number)=>{await input.scrollIntoViewIfNeeded();const b=(await input.boundingBox())!;await page.keyboard.down('Alt');await page.mouse.move(b.x+25,b.y+12);await page.mouse.down();await page.mouse.move(b.x+25+delta,b.y+12,{steps:6});await wait();};
const release=async()=>{await page.mouse.up();await page.keyboard.up('Alt');await wait();};
await begin(30);assert((await doc()).components.size[key].w===200,'preview leaked into save');assert((await view()).includes('w="230"'),'preview missing');await release();assert((await doc()).components.size[key].w===230,'scrub did not commit');
await page.keyboard.press('Meta+z');await wait();assert((await doc()).components.size[key].w===200,'scrub needs more than one undo');
await begin(40);await page.keyboard.press('Escape');await release();assert((await doc()).components.size[key].w===200,'Escape committed scrub');
const selected=(await page.evaluate(()=>(window as any).__iron.selection()))[0];
await begin(50);await edit(`<edit><set id="${selected}" size.w="260"/></edit>`);await release();assert((await doc()).components.size[key].w===260,'stale scrub overwrote model edit');
console.log('inspector: preview, single undo, Escape and concurrent edit pass');
// Verify radial paint on the same transformed local bounds.
const gradientId=(await view()).match(/<rect id="([^"]+)" name="gradient sample"/)[1];
await edit(`<edit><set id="${gradientId}" gradient.kind="radial" gradient.x0="0.5" gradient.y0="0.5" gradient.x1="1" gradient.y1="0.5"/></edit>`);
await page.mouse.move(5,5);await wait();await page.screenshot({path:resolve(dir,'radial.png')});
execFileSync('python3',[resolve(import.meta.dir,'appearance-pixels.py'),dir,'radial'],{stdio:'inherit'});

await click(650,110);await page.locator('[data-leaf="gradient.kind"]').waitFor();
await page.screenshot({path:resolve(dir,'inspector.png')});assert(errors.length===0,errors.join('\n'));console.log('no page errors');
}catch(e){console.log("page errors",errors);await page.screenshot({path:resolve(dir,"failed.png")});throw e;}finally{await browser.close();}
