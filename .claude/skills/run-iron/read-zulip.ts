// Read public Linebender Zulip pages through the JavaScript client, without login.
// bun run .claude/skills/run-iron/read-zulip.ts 'https://xi.zulipchat.com/#narrow/search/retained'
// Captures the client's actual message responses, including stable message links.
import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
const url=process.argv[2]||'https://xi.zulipchat.com/#narrow/search/retained';
if(new URL(url).origin!=='https://xi.zulipchat.com')throw new Error('This reader only opens the public Linebender Zulip');
const parts=new URL(url).hash.replace(/^#narrow\//,'').split('/');
const operand=(operator:string)=>{
 const i=parts.indexOf(operator);if(i<0)return undefined;
 return decodeURIComponent(parts[i+1].replace(/\.([0-9a-f]{2})/gi,'%$1'));
};
const topic=process.argv[3]||operand('topic');
const out=resolve(import.meta.dir,'../../../.agents/research/zulip');mkdirSync(out,{recursive:true});
const browser=await chromium.launch({executablePath:process.env.CHROME||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
const page=await browser.newPage();
const responses:any[]=[];const pending:Promise<void>[]=[];
page.on('response',response=>{
 if(new URL(response.url()).origin!=='https://xi.zulipchat.com'||!response.url().includes('/json/'))return;
 pending.push((async()=>{try {responses.push({url:response.url(),status:response.status(),data:await response.json()});}catch{}})());
});
try {
 await page.goto(url,{waitUntil:'domcontentloaded',timeout:45000});
 await page.waitForTimeout(7000);
 await Promise.allSettled(pending);
 // Reuse the real spectator request shape discovered from the JS client.
 // A channels:web-public narrow is required; a bare API request requires login.
 const request=responses.findLast(r=>r.url.includes('/json/messages?'));
 if(!request)throw new Error('The spectator client did not load a message request');
 {
  const fetchUrl=new URL(request.url);fetchUrl.searchParams.set('anchor','newest');
  fetchUrl.searchParams.set('num_before','1000');fetchUrl.searchParams.set('num_after','0');
  const channel=operand('channel')?.match(/^\d+/)?.[0];
  fetchUrl.searchParams.set('narrow',JSON.stringify([{operator:'channels',operand:'web-public'},
   ...(channel?[{operator:'channel',operand:Number(channel)}]:[]),
   ...(topic?[{operator:'topic',operand:topic}]:operand('search')?[{operator:'search',operand:operand('search')}]:[])]));
  const anchor=operand('near')||operand('with');
  if(anchor&&!topic) {
   fetchUrl.searchParams.set('anchor',anchor);fetchUrl.searchParams.set('num_before','500');fetchUrl.searchParams.set('num_after','500');
  }
  const response=await page.request.get(fetchUrl.href);
  const data=await response.json();
  if(!response.ok()||data.result!=='success')throw new Error(`Public message fetch: ${JSON.stringify(data)}`);
  responses.push({url:fetchUrl.href,status:response.status(),data});
 }
 const messages=new Map<number,any>();
 for(const r of responses.slice(-1))for(const m of r.data.messages||[])messages.set(m.id,m);
 const result={url,messages:[...messages.values()].map(m=>({id:m.id,sender:m.sender_full_name,
  channel:m.display_recipient,channel_id:m.stream_id,topic:m.subject,timestamp:m.timestamp,
  content:m.content,link:`https://xi.zulipchat.com/#narrow/channel/${m.stream_id}/near/${m.id}`})),
  body:await page.locator('body').innerText(),responses:responses.filter(r=>r.url.includes('/json/messages?'))};
 writeFileSync(resolve(out,'last-read.json'),JSON.stringify(result,null,2));
 const name=createHash('sha256').update(url+'\n'+(process.argv[3]||'')).digest('hex').slice(0,12);
 writeFileSync(resolve(out,`${name}.json`),JSON.stringify(result,null,2));
 console.log(JSON.stringify({url,messages:result.messages,body:result.body.slice(0,3500),
  endpoints:responses.map(r=>({url:r.url,status:r.status,keys:Object.keys(r.data)}))},null,2));
}finally {await browser.close();}
