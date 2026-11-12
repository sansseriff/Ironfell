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
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Search from "@lucide/svelte/icons/search";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import X from "@lucide/svelte/icons/x";
  import Select from "./Select.svelte";
  import IconButton from "./IconButton.svelte";
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
    doc.command({ op: 'font_previews', names: want }).then((p: Record<string, Preview>) => (previews = { ...previews, ...p })).catch(() => {});
  }
  $effect(() => {
    void shown;
    queueMicrotask(previewVisible);
  });
  // The current family in view, and highlighted, when the list opens.
  $effect(() => {
    if (!list) return;
    const i = shown.findIndex((r) => r.name === value);
    if (i > 4) list.scrollTop = (i - 4) * 36;
    highlighted = Math.max(0, i);
  });

  // A menu's keyboard (as every Select's): the search keeps focus, arrows
  // move the highlight, Enter picks it; typing filters and highlights the
  // first match. The current family keeps its check.
  let highlighted = $state(0);
  async function highlight(i: number) {
    highlighted = Math.max(0, Math.min(shown.length - 1, i));
    await tick();
    list?.querySelector("[data-highlighted]")?.scrollIntoView({ block: "nearest" });
  }
  const page = () => Math.max(1, Math.floor((list?.clientHeight ?? 360) / 36) - 1);
  function onKey(e: KeyboardEvent) {
    const step: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page(), PageUp: -page() };
    if (e.key in step) { e.preventDefault(); void highlight(highlighted + step[e.key]); }
    else if (e.key === "Enter" && shown[highlighted]) { e.preventDefault(); onPick(shown[highlighted].name); }
  }
</script>

<div class="fonts" role="dialog" aria-label="Fonts">
  <header><strong>Fonts</strong><IconButton icon={X} label="Close fonts" onclick={onClose} /></header>
  <div class="tools">
    <div class="ui-field search">
      <Search size={14} strokeWidth={1.75} />
      <input bind:this={search} bind:value={query} placeholder="Search fonts" aria-label="Search fonts" role="combobox" aria-expanded="true" aria-controls="font-families" aria-activedescendant={shown[highlighted] ? `font-${highlighted}` : undefined}
        oninput={() => (highlighted = 0)} onkeydown={onKey} />
      {#if query}<IconButton icon={CircleX} label="Clear search" onclick={() => { query = ""; search?.focus(); }} />{/if}
    </div>
    <Select value={filter} items={filters} onChange={(v) => (filter = v)} label="Font filter" />
  </div>
  <div class="list" id="font-families" bind:this={list} onscroll={previewVisible} role="listbox" aria-label="Font families">
    {#each shown as row, i (row.name)}
      {@const p = previews[row.name]}
      <button class="ui-menu-item row" id="font-{i}" role="option" tabindex="-1" aria-selected={row.name === value} data-highlighted={i === highlighted ? "" : undefined} title={row.name}
        onpointermove={() => (highlighted = i)} onclick={() => onPick(row.name)}>
        <span class="ui-menu-check">{#if row.name === value}<Check size={14} strokeWidth={1.75} />{/if}</span>
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
  /* Inside a `.ui-popover`: its header is the shared one. */
  .fonts { display: flex; flex-direction: column; max-height: min(560px, 80vh); }
  .tools { display: flex; flex-direction: column; gap: 8px; padding: 12px var(--panel-pad); border-bottom: 1px solid var(--border); }
  .search { gap: 6px; padding: 0 2px 0 8px; color: var(--fg-muted); }
  .search input { flex: 1; min-width: 0; height: calc(var(--control-h) - 2px); padding: 0; border: none; outline: none; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); }
  .list { overflow-y: auto; padding: 4px; flex: 1; min-height: 120px; }
  /* A menu item (ui.css), 36 px tall for the face's preview. */
  .row { width: 100%; height: 36px; border: none; font: inherit; text-align: left; }
  .row:not([data-highlighted]) { background: transparent; color: var(--fg); }
  .preview { fill: currentColor; flex: none; max-width: 210px; }
  .plain { font-size: var(--text-lg); }
  .none { margin: 0; padding: 12px; color: var(--fg-muted); font-size: var(--text-md); }
</style>
