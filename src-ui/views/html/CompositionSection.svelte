<script lang="ts">
  import { controllerManager } from "../../controller-manager.svelte";
  import { activeView } from "../active_view.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import PropertyField, { type Property } from "../../lib/ui/PropertyField.svelte";
  import PaintRow from "../../lib/ui/PaintRow.svelte";

  const doc = controllerManager.docClient;
  const entries = $derived(doc.compositions.filter(c => !c.session));
  let followed = $state<string | null>(null);
  $effect(() => {
    const source = activeView.source;
    if (entries.some(c => c.id === source || c.name === source)) followed = source;
  });
  const current = $derived(entries.find(c => c.id === followed || c.name === followed) ?? entries.find(c => c.name === "main") ?? entries[0]);
  type Info = { node: string; components: { kind: string; fields: Property[] }[] };
  let info = $state<Info | null>(null);
  let error = $state("");
  const fields = $derived(info?.node === current?.id ? info.components.find(c => c.kind === "composition")?.fields ?? [] : []);
  const field = (name: string) => fields.find(f => f.field === name);
  const text = (name: string) => field(name)?.text ?? "";

  $effect(() => {
    void doc.generation;
    const id = current?.id;
    info = null;
    error = "";
    if (!id) return;
    return doc.subscribe<Info>({ op: "inspect", node: id }, r => { info = r; }, e => { error = String(e); });
  });
  async function refresh() {
    const id = current?.id, generation = doc.generation;
    if (!id) return;
    const r = await doc.command<Info>({ op: "inspect", node: id });
    if (id === current?.id && generation === doc.generation) info = r;
  }
  async function set(f: Property, value: string) {
    const id = current?.id;
    if (!id) return;
    error = "";
    try {
      if (f.field === "grid_spacing" && !/^\s*(=|\{=)/.test(value) && (!Number.isFinite(Number(value)) || Number(value) <= 0)) {
        throw new Error("Grid spacing must be greater than zero.");
      }
      const r = await doc.command({ op: "inspect_patch", node: id, patch: JSON.stringify({ values: { [f.leaf]: value }, label: "edit composition settings" }) });
      if (!r.ok) throw new Error(r.error);
      await refresh();
    } catch (e) { error = String(e); }
  }
  function choose(name: string, value: string) { const f = field(name); if (f) void set(f, value); }
</script>

{#if current && fields.length}
  <section class="composition-section" aria-label="Composition settings" data-section="composition">
    <header><strong>Composition</strong><span title={current.name ?? current.id}>{current.name ?? current.id}</span></header>
    <p class="notation">{current.dims.toUpperCase()} · {text("notation")}</p>
    {#if current.dims === "2d"}
      <div class="setting"><span>Background</span><Select label="Composition background" value={text("background")} items={[{value: "theme", label: "Match app"}, {value: "custom", label: "Custom color"}]} onChange={v => choose("background", v)} /></div>
      {#if text("background") === "custom" && field("background_color")}
        <PaintRow node={current.id} label="Background" color={field("background_color")!} onSet={set} onRefresh={refresh} onError={e => error = e} />
      {/if}
      <div class="setting"><span>Grid</span><Select label="Composition grid" value={text("grid")} items={[{value: "off", label: "Off"}, {value: "lines", label: "Lines"}, {value: "dots", label: "Dots"}]} onChange={v => choose("grid", v)} /></div>
      {#if text("grid") !== "off" && field("grid_spacing")}
        <PropertyField field={field("grid_spacing")!} node={current.id} label="Grid spacing (px)" onSet={set} onRefresh={refresh} onError={e => error = e} />
        <p class="hint">Visual guide only. Spacing thins out as you zoom out.</p>
      {/if}
    {/if}
    <div class="setting"><span>Run</span><Select label="Composition run mode" value={text("run")} items={[{value: "auto", label: "Auto"}, {value: "always", label: "Always"}, {value: "on_demand", label: "On demand"}]} onChange={v => choose("run", v)} /></div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .composition-section { padding: 12px; border-bottom: 1px solid var(--border); display: flex; flex-direction: column; gap: 8px; }
  header { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  header span { margin-left: auto; color: var(--fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  strong { font-weight: 600; }
  .notation, .hint { margin: 0; font-size: 11px; color: var(--fg-muted); line-height: 1.5; }
  .setting { display: grid; grid-template-columns: 85px minmax(0, 1fr); align-items: center; gap: 8px; font-size: 12px; }
  .setting :global(.ui-select-trigger) { width: 100%; }
  .error { margin: 0; color: var(--danger); font-size: 12px; }
</style>
