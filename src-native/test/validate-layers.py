"""Packaged native rendering controls. Run from repo root with screen unlocked.
GPU readback checks correctness only: readback/PNG work perturbs frame timing.
"""
import json, os, signal, subprocess, time
from pathlib import Path
binary='target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app/Contents/MacOS/iron-native'
root=Path('.agents/latency');base={k:v for k,v in os.environ.items() if not k.startswith('IRON_NATIVE_')};results=[]
for view in ['overlay','hidden','detached']:
 for render,pacing in [('pipelined','display'),('sync','late')]:
  name=f'{view}-{render}-{pacing}';log=root/f'layers-{name}.log';trace=root/f'layers-{name}.json'
  env={**base,'IRON_NATIVE_FILM':'1','IRON_NATIVE_ALWAYS_RENDER':'1','IRON_NATIVE_LAYER_PROBE':'1','IRON_NATIVE_PROBE_MOTION':'1','IRON_NATIVE_PROBE_WEBVIEW':view,'IRON_NATIVE_RENDER_MODE':render,'IRON_NATIVE_PACING':pacing,'IRON_NATIVE_PROBE_CAPTURE_DIR':str(root/'layers'/name),'IRON_NATIVE_PACING_TRACE':str(trace),'IRON_NATIVE_PACING_SECONDS':'20'}
  start=time.monotonic()
  with log.open('w') as f:
   process=subprocess.Popen(['caffeinate','-d','-u',binary],env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
   try:process.wait(timeout=35)
   except subprocess.TimeoutExpired:
    os.killpg(process.pid,signal.SIGTERM);process.wait();raise
  lines=log.read_text().splitlines();captures=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('IRON_NATIVE_LAYER_CAPTURE ')]
  assert process.returncode==0,(name,process.returncode)
  unavailable=sum(not x.get('available',True) for x in captures)
  captures=[x for x in captures if x.get('available',True)]
  state=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('IRON_NATIVE_LAYER_PROBE ')][0]['webviewState']
  assert state['attached']==(view!='detached') and state['hidden']==(view=='hidden'),state
  assert not any('IRON_SURFACE_RESIZE' in x for x in lines[lines.index(next(x for x in lines if x.startswith('IRON_NATIVE_LAYER_PROBE ')))+1:]),name
  assert len(captures)>=12 and all(x['pass'] for x in captures),(name,captures)
  assert not any('panicked at' in x for x in lines),name
  result={'case':name,'checks':len(captures),'passed':sum(x['pass'] for x in captures),'maxSpriteVectorDifferencePx':max(abs(x['spriteInputX']-x['vectorInputX']) for x in captures),'unavailableReadbacks':unavailable,'seconds':round(time.monotonic()-start,2)}
  results.append(result);print(json.dumps(result),flush=True)
  (root/'layer-results.json').write_text(json.dumps(results,indent=2)+'\n')
