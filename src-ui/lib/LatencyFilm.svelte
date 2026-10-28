<script lang="ts">
  import { onMount } from 'svelte';
  import BevyPanel from './BevyPanel.svelte';
  import { controllerManager } from '../controller-manager.svelte';
  import { pixelRatio } from '../runtime/settings.svelte';
  import { isNativeHost } from '../runtime/native_host';

  const layerProbe = new URLSearchParams(location.search).get('layerProbe') === '1';
  const source = '__iron_latency_film__';
  const targetName = '__film_drag_target__';
  const doc = controllerManager.docClient;
  const label = new URLSearchParams(location.search).get('filmLabel')
    ?? (isNativeHost() ? 'Native' : `Web · ${controllerManager.runtimeMode}`);
  let stage: HTMLDivElement;
  let ring: HTMLDivElement;
  let ready = $state(false);
  let error = $state('');
  let busy = $state(false);
  let shape = $state('');
  let composition = '';
  let prepared = -1;
  let alive = true;
  let continuous = $state(false);
  let cameraFps = $state(240);

  type Saved = {
    nodes: Record<string, { ty: string; parent?: string; tombstoned?: number }>;
    facets: { name?: Record<string, { text: string }> };
  };
  function find(saved: Saved, name: string, type: string, parent?: string) {
    return Object.keys(saved.nodes).find(id => saved.nodes[id].tombstoned == null
      && saved.nodes[id].ty === type && saved.facets.name?.[id]?.text === name
      && (parent === undefined || saved.nodes[id].parent === parent));
  }
  function initialRect() {
    const r = stage.getBoundingClientRect(), k = pixelRatio();
    return { x: Math.max(120, r.width / 2 - 24) * k,
      y: Math.max(40, r.height / 2 - 24) * k, w: 48 * k, h: 48 * k };
  }
  async function prepare(generation: number, reset = false) {
    busy = true; error = ''; if (!reset) ready = false;
    try {
      const saved: Saved = JSON.parse(await doc.save());
      if (!alive || doc.generation !== generation) return;
      composition = find(saved, source, 'composition') ?? '';
      shape = composition ? find(saved, targetName, 'rect', composition) ?? '' : '';
      const r = initialRect();
      const createComposition = composition ? '' : `<create id="$film" type="composition" name="${source}" dims="2d" notation="free"/>`;
      const target = composition || '$film';
      const createShape = shape ? '' : `<create id="$target" type="rect" parent="${target}" name="${targetName}" x="${r.x}" y="${r.y}" w="${r.w}" h="${r.h}" color="#ffd600"/>`;
      const resetShape = shape && reset ? `<set id="${shape}" transform2d.x="${r.x}" transform2d.y="${r.y}" size.w="${r.w}" size.h="${r.h}" transform2d.rot="0"/>` : '';
      if (createComposition || createShape || resetShape) {
        const result = await doc.edit(`<edit>${createComposition}${createShape}${resetShape}</edit>`, 'latency filming fixture');
        if (!result.ok) throw new Error(result.failure ?? JSON.stringify(result));
        if (!alive || doc.generation !== generation) return;
        composition ||= result.aliases.$film;
        shape ||= result.aliases.$target;
      }
      await doc.command({ op: 'snapping', enabled: false });
      const info = await doc.command<{run:string}>({ op: 'composition_info', source: composition });
      if (!alive || doc.generation !== generation) return;
      continuous = info.run === 'always';
      doc.setTool('select');
      controllerManager.syncAllPanels();
      ready = true;
    } catch (e) { if (alive && doc.generation === generation) error = String(e); }
    finally { if (alive && doc.generation === generation) busy = false; }
  }
  $effect(() => {
    // enginePrepared precedes Bevy's first Startup schedule in a fresh browser.
    // Wait for the starting transaction before allocating the fixture's IDs.
    const initialized = controllerManager.isInitialized, generation = doc.generation, version = doc.version;
    if (initialized && version > 0 && stage && prepared !== generation) {
      prepared = generation;
      void prepare(generation);
    }
  });
  async function setContinuous(event: Event) {
    const next = (event.currentTarget as HTMLInputElement).checked;
    try {
      const result = await doc.command<{ok:boolean;error?:string}>({op:'set_run_mode',source:composition,mode:next?'always':'auto'});
      if (!result.ok) throw new Error(result.error ?? 'Could not change run mode');
      continuous = next;
    } catch (e) { error = String(e); }
  }
  async function exportSetup() {
    try {
    const build = await doc.command({op:'build_info'});
    const rect = stage.getBoundingClientRect();
    const setup = { schema:1, capturedAt:new Date().toISOString(), label,
      build, runtime:controllerManager.runtimeMode, url:location.href,
      targetColorsSrgb:{dom:'#00cce7',engine:'#ff00aa',shape:'#ffd600'},
      cameraFps, cameraFrameMs:1000/cameraFps, devicePixelRatio:window.devicePixelRatio,
      renderScale:pixelRatio(), viewportCss:[innerWidth,innerHeight],
      stageCss:{x:rect.x,y:rect.y,width:rect.width,height:rect.height},
      offsetsCss:{domX:-96,engineX:96}, shape, composition, continuous,
      measurement:'Video compares displayed trajectories relative to the OS cursor; not absolute mouse-to-photon without an external physical reference.' };
    const url = URL.createObjectURL(new Blob([JSON.stringify(setup,null,2)],{type:'application/json'}));
    const a = document.createElement('a'); a.href=url; a.download='iron-latency-film-setup.json'; a.click();
    setTimeout(()=>URL.revokeObjectURL(url),1000);
    } catch (e) { error=String(e); }
  }
  onMount(() => {
    const move = (event: PointerEvent) => {
      const events = event.getCoalescedEvents?.();
      const e = events?.length ? events[events.length-1] : event;
      const rect = stage.getBoundingClientRect();
      const inside = e.clientX >= rect.left && e.clientX <= rect.right && e.clientY >= rect.top && e.clientY <= rect.bottom;
      ring.style.visibility = inside ? 'visible' : 'hidden';
      // Deliberately update in the event handler: no Svelte update or rAF wait.
      ring.style.transform = `translate3d(${e.clientX-96-12}px,${e.clientY-12}px,0)`;
    };
    const leave = () => { ring.style.visibility='hidden'; };
    window.addEventListener('pointermove',move,{capture:true,passive:true});
    document.documentElement.addEventListener('pointerleave',leave);
    return () => {
      alive=false;
      window.removeEventListener('pointermove',move,true);
      document.documentElement.removeEventListener('pointerleave',leave);
    };
  });
</script>

<section class="film" data-latency-film data-ready={ready && !busy} data-shape={shape}>
  <header>
    <div class="heading"><strong>Iron · latency filming</strong><span>{label}</span>
      <button disabled={busy || !controllerManager.isInitialized} onclick={() => prepare(doc.generation,true)}>Reset target</button>
      <label><input type="checkbox" checked={continuous} disabled={!ready || busy} onchange={setContinuous}/> Continuous rendering</label>
      <label>Camera fps <input class="fps" type="number" min="30" max="10000" bind:value={cameraFps}/></label>
      <button disabled={!ready || busy || !cameraFps || cameraFps < 30} onclick={exportSetup}>Export setup</button>
    </div>
    <div class="legend">{#if layerProbe}<span class="green">◆ Bevy sprite −192 px</span>{/if}<span class="cyan">○ DOM −96 px</span><span class="magenta">＋ Engine +96 px</span><span class="yellow">■ Normal document drag</span></div>
    <p>Grab the yellow square at its centre and drag horizontally along the guide. Keep the OS cursor visible. Film the screen at 240 fps or faster; use the same window size and monitor for each run.</p>
    <p class="note">Track the target centres and subtract their fixed offsets. This measures extra delay behind the cursor; absolute mouse-to-photon needs a physical input reference. No latency is inferred from software timers.</p>
    <p class="state" class:error={!!error} role={error ? 'alert' : 'status'}>{error || (ready ? 'Ready. Reset after resizing or changing display scale.' : 'Preparing filming scene…')}</p>
  </header>
  <div class="test-stage" bind:this={stage}>
    {#if ready}<BevyPanel id="latency-film" kind="composition" composition={source}/>{/if}
    <div class="guide" aria-hidden="true"></div>
  </div>
  <div class="dom-ring" bind:this={ring} aria-hidden="true"></div>
</section>

<style>
  .film{height:100%;display:flex;flex-direction:column;color:#f4f4f4;font:14px/1.4 system-ui,sans-serif;pointer-events:none}
  header{background:#17191d;padding:12px 20px;flex:none;border-bottom:1px solid #555;pointer-events:auto}
  .heading,.legend{display:flex;gap:18px;align-items:center;flex-wrap:wrap}
  .heading span{color:#bbb}.legend{margin-top:8px;font-weight:700}
  p{margin:6px 0 0}.note{font-size:12px;color:#b9bec8}
  button{background:#343943;border:1px solid #687080;border-radius:4px;padding:5px 9px;color:white;cursor:pointer}
  button:disabled{opacity:.5;cursor:default}label{display:flex;align-items:center;gap:5px}.fps{width:66px;color:black;background:white}
  .green{color:#00d232}.cyan{color:#00cce7}.magenta{color:#ff00aa}.yellow{color:#ffd600}.error{color:#ff8e8e}
  .test-stage{position:relative;flex:1;min-height:120px;overflow:hidden}
  .guide{position:absolute;top:50%;left:0;right:0;border-top:1px dashed #8888;pointer-events:none}
  .dom-ring{position:fixed;left:0;top:0;width:24px;height:24px;box-sizing:border-box;border:4px solid #00cce7;border-radius:50%;outline:2px solid black;visibility:hidden;pointer-events:none;z-index:2;will-change:transform}
</style>
