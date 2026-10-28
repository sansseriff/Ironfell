(async()=>{
 const core=window.__TAURI__.core,sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const report={checks:[],errors:[]};
 window.addEventListener('error',e=>report.errors.push(e.message));
 window.addEventListener('unhandledrejection',e=>report.errors.push(String(e.reason)));
 const check=(name,condition,detail)=>{report.checks.push({name,pass:!!condition,detail});if(!condition)throw Error(name+': '+JSON.stringify(detail));};
 const ready=async()=>{for(let i=0;i<200;i++){if(document.querySelector('[data-latency-film]')?.dataset.ready==='true')return;await sleep(100);}throw Error('filming scene not ready');};
 const snapshot=async()=>{const d=JSON.parse(await __iron.save());const shape=document.querySelector('[data-latency-film]').dataset.shape;const compositions=Object.keys(d.nodes).filter(id=>d.nodes[id].tombstoned==null&&d.facets.name?.[id]?.text==='__iron_latency_film__');return{shape,compositions,document:d,t:d.facets.transform2d[shape]};};
 try{
  await ready();await sleep(500);
  const s=await snapshot();
  check('one diagnostic composition',s.compositions.length===1,s.compositions);
  check('normal document target',s.document.nodes[s.shape].ty==='rect',s.shape);
  check('target uses CSS-sized dimensions',s.document.facets.size[s.shape].w===48*devicePixelRatio);
  const carry=sessionStorage.getItem('iron.film.reload');
  if(carry){
   sessionStorage.removeItem('iron.film.reload');const old=JSON.parse(carry);
   report.checks=old.report.checks;report.errors.push(...old.report.errors);
   check('reload reuses target ID',s.shape===old.shape);
   check('reload preserves authored document without extra fixture ops',await __iron.save()===old.saved);
  }else{
   __probe();await sleep(200);const p=window.__lastProbeStats;
   check('native pointer path active',p.pointerSource==='native',p);
   check('render mode active',p.renderThreadEnabled===(p.requestedRenderMode==='pipelined'));
   check('native mode caption',document.querySelector('.heading').textContent.includes(p.requestedRenderMode+' / '+p.pacing));
   const edit=await __iron.edit(`<edit><set id="${s.shape}" x="${s.t.x+80}"/></edit>`);
   check('target is editable through shared transactions',edit.ok,edit);
   document.querySelector('.heading button').click();await sleep(50);await ready();
   const reset=await snapshot(),r=document.querySelector('.test-stage').getBoundingClientRect();
   check('reset aligns target with horizontal guide',Math.abs((reset.t.y+24*devicePixelRatio)/devicePixelRatio-r.height/2)<1,{y:reset.t.y,height:r.height,dpr:devicePixelRatio});
   check('reset creates no duplicate composition',reset.compositions.length===1);
   const toggle=document.querySelector('.heading input[type=checkbox]');toggle.click();await sleep(300);
   check('continuous mode uses composition run contract',(await __iron.compositionInfo(reset.compositions[0])).run==='always');
   toggle.click();await sleep(300);
   check('on-demand mode can be restored',(await __iron.compositionInfo(reset.compositions[0])).run==='auto');
   sessionStorage.setItem('iron.film.reload',JSON.stringify({report,shape:s.shape,saved:await __iron.save()}));
   location.reload();return;
  }
  check('no page errors',report.errors.length===0,report.errors);
 }catch(e){report.failure=String(e);}
 await core.invoke('native_test',{action:'report',report});
 await core.invoke('native_test',{action:'close'});
})();
