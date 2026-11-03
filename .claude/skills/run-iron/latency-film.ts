/** Filming-mode integration checks; run with the worktree's server at URL. */
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
const out = '../../../.agents/latency';
mkdirSync(out,{recursive:true});
const browser = await chromium.launch({ executablePath:process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-angle=metal','--enable-gpu','--ignore-gpu-blocklist','--no-sandbox'] });
const url = new URL(process.env.URL ?? 'http://127.0.0.1:5185/Ironfell/?gfx=webgl2&demo');
url.searchParams.set('latencyFilm','1');
const backend=url.searchParams.get('gfx') ?? 'auto';
const images: unknown[]=[];
try {
 for (const [runtime,dpr] of [['worker',1],['worker',2],['main',1]] as const) {
  const context=await browser.newContext({viewport:{width:1280,height:850},deviceScaleFactor:dpr});
  await context.addInitScript(runtime=>localStorage.setItem('iron.settings.v1',JSON.stringify({runtime,renderScale:'auto'})),runtime);
  const page=await context.newPage();const errors:string[]=[];
  page.on('pageerror',e=>errors.push(String(e)));
  page.on('console',m=>{if(m.text().includes('panicked at'))errors.push(m.text());});
  let phase="initial"; console.log({runtime,dpr,phase}); await page.goto(url.toString());
  const ready=async()=>{try{await page.waitForFunction(()=>document.querySelector('[data-latency-film]')?.getAttribute('data-ready')==='true',{},{timeout:30000});}catch(e){console.log({runtime,dpr,phase,errors,body:await page.locator('body').innerText()});throw e;}};
  await ready();
  const snapshot=()=>page.evaluate(async()=>{const w=window as any,d=JSON.parse(await w.__iron.save());const el=document.querySelector('[data-latency-film]')!;const id=el.getAttribute('data-shape')!;const c=Object.keys(d.nodes).filter(i=>d.nodes[i].tombstoned==null&&d.facets.name?.[i]?.text==='__iron_latency_film__');return {id,compositions:c,t:d.facets.transform2d[id],size:d.facets.size[id],version:d.version};});
  const before=await snapshot();assert.equal(before.compositions.length,1);assert.equal(before.size.w,48*dpr);
  const rect=await page.locator('.test-stage').boundingBox();assert.ok(rect);
  const x=rect.x+(before.t.x+before.size.w/2)/dpr,y=rect.y+(before.t.y+before.size.h/2)/dpr;
  assert.ok(Math.abs(y-(rect.y+rect.height/2))<1,'Target must align with the guide after ready');
  await page.mouse.move(x,y);await page.waitForTimeout(250);
  const file=`${out}/film-${backend}-${runtime}-dpr${dpr}.png`;
  await page.screenshot({path:file});
  images.push({file:`.agents/latency/film-${backend}-${runtime}-dpr${dpr}.png`,dpr,stage:rect,pointer:{x,y}});
  await page.mouse.down();
  for(let n=1;n<=12;n++){await page.mouse.move(x+n*10,y);await page.waitForTimeout(20);}
  await page.mouse.up();await page.waitForTimeout(250);
  const after=await snapshot();assert.ok(Math.abs(after.t.x-before.t.x-120*dpr)<4*dpr,'Normal drag must move the authored target');
  await page.keyboard.press('Meta+z');await page.waitForTimeout(250);
  const undone=await snapshot();assert.equal(undone.t.x,before.t.x);assert.equal(undone.t.y,before.t.y);
  await page.getByText('Continuous rendering',{exact:true}).click();
  assert.equal((await page.evaluate(async(id)=>await (window as any).__iron.compositionInfo(id),before.compositions[0])).run,'always');
  await page.getByText('Continuous rendering',{exact:true}).click();
  const downloadPromise=page.waitForEvent('download');await page.getByRole('button',{name:'Export setup'}).click();
  const download=await downloadPromise;const path=await download.path();assert.ok(path);
  const metadata=JSON.parse(await Bun.file(path).text());assert.equal(metadata.offsetsCss.domX,-96);assert.equal(metadata.offsetsCss.engineX,96);assert.equal(metadata.renderScale,dpr);assert.equal(metadata.build.gfx,url.searchParams.get('gfx')); 
  phase="reload";console.log({runtime,dpr,phase}); const beforeReload=await snapshot();await page.reload();await ready();const reloaded=await snapshot();
  assert.equal(reloaded.id,before.id);assert.equal(reloaded.compositions.length,1);assert.equal(reloaded.t.x,before.t.x);assert.equal(reloaded.t.y,before.t.y);
  phase='reset';console.log({runtime,dpr,phase});await page.getByRole('button',{name:'Reset target'}).click();await ready();assert.equal((await snapshot()).compositions.length,1);
  assert.equal(await page.getByRole("alert").count(),0);assert.deepEqual(errors,[]);
  console.log(JSON.stringify({runtime,dpr,dragUndo:true,reloadHasSingleFixture:true,metadata:true,errors}));
  await context.close();
 }
 writeFileSync(`${out}/film-images-${backend}.json`,JSON.stringify(images,null,2));
}finally{await browser.close();}
