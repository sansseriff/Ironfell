<script lang="ts">
  import { controllerManager } from "../../controller-manager.svelte";
  import { activeView } from "../active_view.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import PropertyField, { type Property } from "../../lib/ui/PropertyField.svelte";
  import PaintRow from "../../lib/ui/PaintRow.svelte";
  import Section from "../../lib/ui/Section.svelte";

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

{#snippet runSetting()}
  <div class="setting"><span class="ui-subheading">Run</span><Select label="Composition run mode" value={text("run")} items={[{value: "auto", label: "Auto"}, {value: "always", label: "Always"}, {value: "on_demand", label: "On demand"}]} onChange={v => choose("run", v)} /></div>
{/snippet}

{#if current && fields.length}
  <Section title="Composition" meta={current.name ?? current.id} aria-label="Composition settings" data-section="composition">
    <p class="notation">{current.dims.toUpperCase()} · {text("notation")}</p>
    <!-- Labels over fields, as in every inspector section. -->
    <div class="property-grid">
      {#if current.dims === "2d"}
        <div class="setting"><span class="ui-subheading">Background</span><Select label="Composition background" value={text("background")} items={[{value: "theme", label: "Match app"}, {value: "custom", label: "Custom color"}]} onChange={v => choose("background", v)} /></div>
      {/if}
      {@render runSetting()}
    </div>
    {#if current.dims === "2d"}
      {#if text("background") === "custom" && field("background_color")}
        <PaintRow node={current.id} label="Background" color={field("background_color")!} onSet={set} onRefresh={refresh} onError={e => error = e} />
      {/if}
      <div class="property-grid">
        <div class="setting"><span class="ui-subheading">Grid</span><Select label="Composition grid" value={text("grid")} items={[{value: "off", label: "Off"}, {value: "lines", label: "Lines"}, {value: "dots", label: "Dots"}]} onChange={v => choose("grid", v)} /></div>
        {#if text("grid") !== "off" && field("grid_spacing")}
          <PropertyField field={field("grid_spacing")!} node={current.id} label="Grid spacing (px)" onSet={set} onRefresh={refresh} onError={e => error = e} />
        {/if}
      </div>
      {#if text("grid") !== "off" && field("grid_spacing")}<p class="hint">Visual guide only. Spacing thins out as you zoom out.</p>{/if}
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </Section>
{/if}

<style>
  .notation, .hint { margin: 0; font-size: var(--text-sm); color: var(--fg-muted); line-height: 1.5; }
  .setting { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .error { margin: 0; color: var(--danger); font-size: var(--text-md); }
</style>
