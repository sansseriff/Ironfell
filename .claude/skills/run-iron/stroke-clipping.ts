// Automatic embed frames preserve painted outsets; explicit frames still clip.
// Run after build.sh + serve.sh start: bun run stroke-clipping.ts (DPR=2 optional).
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const dir=resolve(import.meta.dir,'../../../.agents/stroke-clipping');mkdirSync(dir,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROME??'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox']});
const page=await browser.newPage({viewport:{width:1600,height:1000},colorScheme:'light',deviceScaleFactor:Number(process.env.DPR??1)});
page.setDefaultTimeout(15000);
const errors:string[]=[];page.on('pageerror',e=>{errors.push(String(e));console.log('PAGE ERROR',String(e));});page.on('console',m=>{if(m.text().includes('panicked at'))errors.push(m.text());});
const assert=(ok:unknown,why:string)=>{if(!ok)throw new Error(why);};
const wait=()=>page.waitForTimeout(600);
const view=()=>page.evaluate(()=>(window as any).__iron.view({fidelity:'full'}));
const edit=async(xml:string)=>{const r=await page.evaluate(x=>(window as any).__iron.edit(x),xml);assert(r.ok, JSON.stringify(r));await wait();};
try{
await page.goto('http://localhost:5173/Ironfell/?gfx=webgl2&layout=default');await page.waitForSelector('.loading',{state:'detached',timeout:180000});await page.waitForFunction(async () => !!(window as any).__iron && (await (window as any).__iron.view({fidelity:'full'})).includes('name="controls"'), {timeout:180000});await page.waitForTimeout(1000);
const v=await view(),parent=v.match(/<composition id="([^"]+)" name="controls"/)[1];
await edit(`<edit>
<create id="$source" type="composition" name="stroke bounds source" dims="2d"/>
<create id="$anchor" type="rect" parent="$source" w="1" h="1" color="#00000000"/>
<create id="$rect" type="rect" parent="$source" x="40" y="40" w="240" h="160" color="#8866dd"/>
<add id="$rect" corners.tl="45" corners.tr="45" corners.br="45" corners.bl="45" stroke.color="#000000" stroke.width="16" gradient.kind="linear" gradient.start="#6546ed" gradient.end="#e86cb3"/>
</edit>`);
const source=(await view()).match(/<composition id="([^"]+)" name="stroke bounds source"/)[1];
await edit(`<edit>
<create id="$embed" type="embed" parent="${parent}" source="${source}" x="80" y="60"/>
<create id="$framed" type="embed" parent="${parent}" source="${source}" x="430" y="60" frame.x0="0" frame.y0="0" frame.x1="280" frame.y1="200"/>
</edit>`);
await page.locator('.dv-tab',{hasText:/^\s*Controls\s*$/}).click();await wait();
const p=(await page.locator('[data-panel-id="controls"]').boundingBox())!;
await page.mouse.move(5,5);await wait();await page.screenshot({path:resolve(dir,'stroke-clipping.png')});
writeFileSync(resolve(dir,'stroke-panel.json'),JSON.stringify({...p,dpr:Number(process.env.DPR??1)}));
execFileSync('python3',[resolve(import.meta.dir,'stroke-clipping-pixels.py'),dir],{stdio:'inherit'});
const rect=(await view()).match(/<rect id="([^"]+)"[^>]*x="40" y="40"/)[1];
await edit(`<edit><set id="${rect}" stroke.width="32" size.w="260" size.h="180"/></edit>`);
await page.screenshot({path:resolve(dir,'stroke-clipping-edited.png')});
execFileSync('python3',[resolve(import.meta.dir,'stroke-clipping-pixels.py'),dir,'edited'],{stdio:'inherit'});
assert(errors.length===0,errors.join('\n'));console.log('no page errors');
}catch(e){throw e;}finally{await browser.close();}
