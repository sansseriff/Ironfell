"""Extract a durable summary from saved Apple Metal HUD HTML reports.
Run after the interactive five-second reports and validate-layers.py.
"""
import hashlib,html,json,re
from pathlib import Path
root=Path('.agents/latency');reports=[]
for path in sorted((root/'hud-reports').glob('*.html')):
 text=path.read_text();metrics={}
 for label,avg,last,low,high in re.findall(r'<tr><td>([^<]+)</td><td[^>]*>([^<]+)</td><td[^>]*>([^<]+)</td><td[^>]*>([^<]+)</td><td[^>]*>([^<]+)</td></tr>',text):
  if label in ['Present Delay','Command Buffer GPU','Command Buffer CPU','Frame Interval','FPS']:
   metrics[html.unescape(label)]={'average':avg,'last':last,'min':low,'max':high}
 assert 'Present Delay' in metrics,path
 log=root/f'present-{path.stem}.log'
 lines=log.read_text().splitlines() if log.exists() else []
 def event(prefix):
  return next((json.loads(s.split(' ',1)[1]) for s in lines if s.startswith(prefix+' ')),None)
 reports.append({'case':path.stem,'source':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
  'frames':int(re.search(r'Total Frames</span> <span>(\d+)',text)[1]),
  'durationSeconds':float(re.search(r'Total Frames</span> <span>\d+</span> \(([\d.]+)s\)',text)[1]),
  'metrics':metrics,'nativePresentation':event('IRON_NATIVE_PRESENTATION'),'layerProbe':event('IRON_NATIVE_LAYER_PROBE')})
record={'date':'2026-10-27','baseCommit':'65157a1','worktree':'native-latency','hardware':'Apple M5, 60 Hz display, macOS 27.2 as reported by Metal HUD',
 'method':'Same small filming fixture, forced continuous rendering, scripted engine pointer positions; no GPU readback during HUD timing reports. Each completed HUD report covers five seconds. Frame CPU traces span longer intervals and must not be added directly to these report averages.',
 'metricDefinition':'Apple Metal HUD Present Delay: presentDrawable call to reported display presentation. Not physical input-to-photon or LCD pixel response.',
 'sources':['https://developer.apple.com/documentation/xcode/understanding-metal-performance-hud-metrics','https://developer.apple.com/documentation/xcode/customizing-metal-performance-hud'],
 'reports':reports,
 'gpuReadbackControls':json.loads((root/'layer-results.json').read_text()) if (root/'layer-results.json').exists() else [],
 'observations':[{'case':'overlay-pipelined-display','source':'CUA-inspected live HUD only; report did not complete','presentDelayRollingMs':45.79,'gpuRollingMs':2.33,'composition':'Composited'},
 {'case':'overlay-sync-display-immediate','source':'CUA-inspected live HUD plus completed report','composition':'Direct','displaySyncEnabled':False}],
 'limitations':['Exploratory single-session measurements, not randomized repeated trials. OS/HUD instrumentation and background workload may affect numbers.',
 'No completed saved HUD report for pipelined/display or sync/display FIFO; exclude these cases from quantitative report comparisons.',
 'An early detached run changed viewport height from 1636 to 1700; excluded. Final detached control freezes 2560x1636 before removing the webview.',
 'Some pipelined surface screenshot readbacks are fully transparent; unavailable captures are excluded/retried, not counted as stale imagery. Readback tests force continuous rendering and are not timing captures.',
 'Queue-completion callbacks include device polling delay and are not GPU or presentation timestamps.',
 'Standalone winit/Bevy comparative latency and CAMetalDisplayLink external-drawable integration have not been measured. The later standalone control is built; no speedup or 3D latency result is inferred.'],
 'conclusions':['The Vello reference and a direct Bevy sprite show the same requested sample in valid controlled GPU readbacks; no extra Vello frame was observed.',
 'Tens of milliseconds of post-submit delay remain with a detached webview and with native display sync disabled.',
 'Fewer drawables reduce reported post-submit delay but increase CPU acquisition waits; end-to-end input improvement is not established.',
 'The sync/late CPU timing improvement does not establish lower displayed drag latency; scheduling against CADisplayLink targets does not by itself control the drawable presentation queue.']}
path=Path('plans/rendering/benchmarks/native-presentation-2026-10-27.json');path.write_text(json.dumps(record,indent=2)+'\n')
for r in reports:print(r['case'],r['frames'],'frames','present',r['metrics']['Present Delay']['average'],'GPU',r['metrics']['Command Buffer GPU']['average'])
