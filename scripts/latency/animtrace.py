# Per vsync, from a Chrome trace (latency-anim.ts TRACE=1): did the worker's
# frame reach this vsync's draw (M) or miss it (x), and how often that flips;
# worker frames dispatched per vsync; when the page's frame was drawn and the
# worker's dispatched, after BeginFrame; tick timing.
# plans/rendering/worker-frame-hold.md §6f-§6g.
#   python3 scripts/latency/animtrace.py .agents/latency/anim/*-trace.json
import json, sys, statistics as st, collections
def q(v,p): v=sorted(v); return v[min(len(v)-1,int(len(v)*p))] if v else float('nan')
for n in sys.argv[1:]:
    ev=json.load(open(n))['traceEvents']
    th={(e['pid'],e['tid']):e['args']['name'] for e in ev if e.get('ph')=='M' and e.get('name')=='thread_name'}
    def X(name, thread=None, end=False):
        return sorted((e['ts']+(e.get('dur',0) if end else 0)) for e in ev if e.get('name')==name and e.get('ph')=='X' and (thread is None or th.get((e['pid'],e['tid']))==thread))
    B=X('DisplayScheduler::BeginFrame','VizCompositorThread'); B=B[30:-30]
    draws=X('DisplayScheduler::DrawAndSwap','VizCompositorThread')
    disp=X('CanvasResourceDispatcher::DispatchFrame','DedicatedWorker thread',end=True)
    wr=[(e['ts'],e.get('dur',0)) for e in ev if e.get('name')=='FireAnimationFrame' and e.get('ph')=='X' and th.get((e['pid'],e['tid']))=='DedicatedWorker thread']
    seq=[]; per=collections.Counter(); drawt=[]; dispt=[]
    for a,b in zip(B,B[1:]):
        d=[x for x in draws if a<=x<b]; w=[x for x in disp if a<=x<b]
        per[len(w)]+=1
        if d: drawt.append((d[0]-a)/1000)
        if w: dispt.append((w[0]-a)/1000)
        if d and w: seq.append('M' if w[0]<d[0] else 'x')
        elif d: seq.append('-')
    s=''.join(seq); flips=sum(1 for i in range(1,len(s)) if s[i]!=s[i-1] and '-' not in s[i-1:i+1])
    ticks=[d/1000 for t,d in wr if d>1000]
    name=n.split('/')[-1].replace('-trace.json','')
    print(f"{name}: vsyncs {len(B)-1}; worker frames per vsync {dict(sorted(per.items()))}; made the draw {s.count('M')}, missed {s.count('x')}, none sent {s.count('-')}; flips {flips}")
    print(f"   draw after BeginFrame p10/p50/p90 {q(drawt,.1):.2f}/{q(drawt,.5):.2f}/{q(drawt,.9):.2f} ms; worker dispatch p10/p50/p90 {q(dispt,.1):.2f}/{q(dispt,.5):.2f}/{q(dispt,.9):.2f} ms; tick p50/p90/max {q(ticks,.5):.2f}/{q(ticks,.9):.2f}/{max(ticks) if ticks else 0:.1f} ms")
    print('   ', s[:150])
