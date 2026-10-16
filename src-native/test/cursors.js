(async () => {
  const core=window.__TAURI__.core;
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const report={checks:[],violations:[]};
  document.addEventListener('securitypolicyviolation',e=>report.violations.push({directive:e.effectiveDirective,uri:e.blockedURI}));
  const image=url=>new Promise(resolve=>{const i=new Image();i.onload=()=>resolve({ok:true,width:i.naturalWidth,height:i.naturalHeight});i.onerror=()=>resolve({ok:false});i.src=url;});
  try {
    for(let i=0;i<300&&!window.__iron;i++) await sleep(100);
    await sleep(3000);
    const canvas=document.getElementById('bevy-canvas');
    canvas.focus();
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'p',bubbles:true}));
    window.dispatchEvent(new KeyboardEvent('keyup',{key:'p',bubbles:true}));
    await sleep(400);
    const style=canvas.style.cursor, computed=getComputedStyle(canvas).cursor;
    const describe=s=>s.replace(/data:image\/png;base64,[^"]+/g,s=>`data:image/png;[${s.length} characters]`);
    report.cursor={name:canvas.dataset.cursor,style:describe(style),computed:describe(computed)};
    const url=style.match(/url\("([^"]+)"\)/)?.[1];
    if(url)report.fixedImage=await image(url);
    const png=document.createElement('canvas');png.width=png.height=32;
    png.getContext('2d').fillRect(0,0,16,16);
    report.generatedImage=await image(png.toDataURL());
    report.checks.push({name:'engine pen cursor reaches shared CSS adapter',pass:canvas.dataset.cursor==='pen'});
    report.checks.push({name:'packaged fixed cursor loads',pass:report.fixedImage?.ok===true});
    report.checks.push({name:'native cursor uses 64px Retina raster',pass:report.fixedImage?.width===64});
    report.checks.push({name:'pen keeps useful keyword fallback',pass:computed.endsWith('crosshair')});
    report.checks.push({name:'generated cursor image permitted',pass:report.generatedImage.ok});
    report.checks.push({name:'no image CSP violations',pass:report.violations.length===0});
  } catch(e){report.failure=String(e);}
  await core.invoke('native_test',{action:'report',report});
  await core.invoke('native_test',{action:'close'});
})();
