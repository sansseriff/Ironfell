<script lang="ts" module>
  export type FontFamily = { name: string; file: string; category: string; weights: [number, number] };
  export type FontCatalog = { default: string; families: FontFamily[]; weights: { weight: number; name: string }[]; system: string[]; loaded: string[] };
  type Preview = { d: string; w: number; h: number };

  // The catalog's previews, shared by every picker: one small file of SVG
  // paths (`public/fonts/previews.json`, made by iron_text's font_previews),
  // so the list shows each face without fetching a single font file.
  let catalogPreviews: Promise<Record<string, Preview>> | null = null;
  function loadPreviews(): Promise<Record<string, Preview>> {
    catalogPreviews ??= fetch(`${import.meta.env.BASE_URL}fonts/previews.json`)
      .then((r) => (r.ok ? r.json() : { families: {} }))
      .then((j) => j.families ?? {})
      .catch(() => ({}));
    return catalogPreviews;
  }
</script>

<script lang="ts">
  // Figma's font list (plans/specs/text.md §4): search, a filter, and every
  // family drawn in its own face. Catalog families come with the app;
  // installed ones appear on the native host, previewed by the engine as
  // they scroll into view.
  import Check from "@lucide/svelte/icons/check";
  import Search from "@lucide/svelte/icons/search";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import X from "@lucide/svelte/icons/x";
  import Select from "./Select.svelte";
  import { controllerManager } from "../../controller-manager.svelte";

  let { value, catalog, onPick, onClose }: { value: string; catalog: FontCatalog; onPick: (family: string) => void; onClose: () => void } = $props();

  const doc = controllerManager.docClient;
  let query = $state("");
  let filter = $state("all");
  let previews: Record<string, Preview> = $state({});
  let search: HTMLInputElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  $effect(() => {
    void loadPreviews().then((p) => (previews = { ...p, ...previews }));
    search?.focus();
    search?.select();
  });

  type Row = { name: string; category: string; installed: boolean };
  const rows: Row[] = $derived.by(() => {
    // Fallback faces (emoji) load by themselves and are never picked.
    const own = catalog.families.filter((f) => f.category !== "emoji").map((f) => ({ name: f.name, category: f.category, installed: false }));
    const names = new Set(own.map((r) => r.name.toLowerCase()));
    const sys = catalog.system.filter((n) => !names.has(n.toLowerCase())).map((n) => ({ name: n, category: "installed", installed: true }));
    return [...own, ...sys].sort((a, b) => a.name.localeCompare(b.name));
  });
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => (filter === "all" || r.category === filter) && (!q || r.name.toLowerCase().includes(q)));
  });
  const filters = $derived([
    { value: "all", label: "All fonts" },
    { value: "sans", label: "Sans serif" },
    { value: "serif", label: "Serif" },
    { value: "mono", label: "Monospace" },
    { value: "handwriting", label: "Handwriting" },
    ...(catalog.system.length ? [{ value: "installed", label: "Installed" }] : []),
  ]);

  // Installed families are previewed by the engine, a page at a time, as
  // the list scrolls to them.
  let asked = new Set<string>();
  function previewVisible() {
    if (!list) return;
    const rowH = 36;
    const first = Math.floor(list.scrollTop / rowH);
    const want = shown.slice(first, first + Math.ceil(list.clientHeight / rowH) + 10).filter((r) => r.installed && !previews[r.name] && !asked.has(r.name)).map((r) => r.name);
    if (!want.length) return;
    want.forEach((n) => asked.add(n));
    doc.call("font_previews", [want]).then((p: Record<string, Preview>) => (previews = { ...previews, ...p })).catch(() => {});
  }
  $effect(() => {
    void shown;
    queueMicrotask(previewVisible);
  });
  // The current family in view when the list opens.
  $effect(() => {
    if (!list) return;
    const i = shown.findIndex((r) => r.name === value);
    if (i > 4) list.scrollTop = (i - 4) * 36;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && shown.length) onPick(shown[0].name);
  }
</script>

<div class="fonts" role="dialog" aria-label="Fonts">
  <header><strong>Fonts</strong><button class="property-icon" aria-label="Close fonts" onclick={onClose}><X size={16} strokeWidth={1.5} /></button></header>
  <div class="search">
    <Search size={15} strokeWidth={1.75} />
    <input bind:this={search} bind:value={query} placeholder="Search fonts" aria-label="Search fonts" onkeydown={onKey} />
    {#if query}<button class="clear" aria-label="Clear search" onclick={() => { query = ""; search?.focus(); }}><CircleX size={15} strokeWidth={1.5} /></button>{/if}
  </div>
  <div class="filter"><Select value={filter} items={filters} onChange={(v) => (filter = v)} label="Font filter" /></div>
  <div class="list" bind:this={list} onscroll={previewVisible} role="listbox" aria-label="Font families">
    {#each shown as row (row.name)}
      {@const p = previews[row.name]}
      <button class="row" class:current={row.name === value} role="option" aria-selected={row.name === value} title={row.name} onclick={() => onPick(row.name)}>
        <span class="check">{#if row.name === value}<Check size={15} strokeWidth={2} />{/if}</span>
        {#if p}
          <svg class="preview" viewBox="0 0 {p.w} {p.h}" width={(p.w * 20) / p.h} height="20" aria-hidden="true"><path d={p.d} /></svg>
        {:else}
          <span class="plain">{row.name}</span>
        {/if}
      </button>
    {:else}
      <p class="none">No fonts match “{query}”</p>
    {/each}
  </div>
</div>

<style>
  .fonts { width: 260px; display: flex; flex-direction: column; max-height: min(560px, 80vh); }
  header { display: flex; justify-content: space-between; align-items: center; padding: 14px 12px 10px 16px; font-size: 13px; }
  .search { display: flex; align-items: center; gap: 6px; margin: 0 10px; padding: 0 8px; height: 32px; border: 1px solid var(--accent); border-radius: 6px; background: var(--bg-sunken); color: var(--fg-muted); }
  .search input { flex: 1; min-width: 0; border: none; outline: none; background: transparent; color: var(--fg); font: inherit; font-size: 13px; padding: 0; }
  .clear { display: flex; border: none; background: transparent; color: var(--fg-muted); padding: 0; cursor: pointer; }
  .filter { padding: 10px; border-bottom: 1px solid var(--border); }
  .filter :global(.ui-select-trigger) { width: 100%; }
  .list { overflow-y: auto; padding: 4px 0 8px; flex: 1; min-height: 120px; }
  .row { display: flex; align-items: center; gap: 6px; width: 100%; height: 36px; padding: 0 12px 0 8px; border: none; background: transparent; color: var(--fg); cursor: pointer; text-align: left; }
  .row:hover { background: var(--hover); }
  .row.current { background: var(--hover); }
  .check { width: 18px; display: flex; justify-content: center; flex: none; }
  .preview { fill: currentColor; flex: none; max-width: 210px; }
  .plain { font-size: 15px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .none { padding: 12px 16px; color: var(--fg-faint); font-size: 12px; }
</style>
