<script lang="ts">
  // The scopes page (route.ts, plans/specs/scopes.md §10.2): every stored
  // scope, opened by a click. It reads browser storage only; the engine's
  // wasm and the app's code are fetched in the background once the page is
  // idle (warm_engine.ts), and opening a scope swaps to the app in place
  // (`openApp`, main.ts), so they are used as fetched. New, New demo and an
  // Export that needs the log replayed open the app to do it.
  import { onMount } from "svelte";
  import { AlertDialog, Button as BitsButton, DropdownMenu } from "bits-ui";
  import Plus from "@lucide/svelte/icons/plus";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Upload from "@lucide/svelte/icons/upload";
  import Download from "@lucide/svelte/icons/download";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Copy from "@lucide/svelte/icons/copy";
  import Trash from "@lucide/svelte/icons/trash-2";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Database from "@lucide/svelte/icons/database";
  import Lock from "@lucide/svelte/icons/lock";
  import Button from "./lib/ui/Button.svelte";
  import EmptyState from "./lib/ui/EmptyState.svelte";
  import TextInput from "./lib/ui/TextInput.svelte";
  import { OpfsScopeStorage, type ScopeMeta } from "./runtime/scope_storage";
  import { ScopeLibrary, webLocks } from "./runtime/scopes";
  import { newScopeUrl, scopeUrl } from "./route";
  import { warmEngine } from "./runtime/warm_engine";

  let { openApp }: { openApp: (url: string) => Promise<void> } = $props();

  type Row = ScopeMeta & { bytes: number; busy: boolean };

  const library = new ScopeLibrary(new OpfsScopeStorage(), webLocks());
  let rows: Row[] = $state([]);
  let loaded = $state(false);
  let storage: { usage: number; quota: number } | null = $state(null);
  let error = $state("");
  let renaming: string | null = $state(null);
  let draft = $state("");
  let deleting: Row | null = $state(null);
  let fileInput: HTMLInputElement | undefined = $state();

  async function refresh() {
    try {
      const list = await library.list();
      rows = await Promise.all(list.map(async (m) => ({ ...m, busy: await library.busy(m.id) })));
      storage = await library.estimate();
    } catch (e) {
      error = String(e);
    }
    loaded = true;
  }
  onMount(() => {
    document.title = "Scopes";
    void refresh();
    void warmEngine();
    // Another tab may open, close or change scopes meanwhile.
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function act(work: () => Promise<unknown>) {
    error = "";
    try {
      await work();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await refresh();
  }

  const open = (id: string) => void openApp(scopeUrl(id));
  const create = (kind: "empty" | "demo") => void openApp(newScopeUrl(kind));

  function startRename(r: Row) {
    renaming = r.id;
    draft = r.name;
  }
  function commitRename() {
    const id = renaming;
    renaming = null;
    if (id) void act(() => library.rename(id, draft));
  }

  async function exportScope(r: Row) {
    const out = await library.exportText(r.id);
    // Its log needs replaying into one document: the app does it.
    if (!out) return openApp(scopeUrl(r.id, true));
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([out.text], { type: "application/json" }));
    a.download = `${out.name.replace(/[^\w.-]+/g, "-") || "iron-document"}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  async function onFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (file) await act(async () => library.importText(await file.text(), file.name.replace(/\.json$/i, "")));
  }

  function bytes(n: number) {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(1)} GB`;
  }
  function ago(ms: number) {
    const s = (Date.now() - ms) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return new Date(ms).toLocaleDateString();
  }
</script>

<main class="scopes-page" data-scopes-page>
  <header>
    <div class="heading">
      <Database size={18} strokeWidth={1.75} />
      <h1>Scopes</h1>
    </div>
    <div class="actions">
      <Button variant="outline" onclick={() => fileInput?.click()} data-action="import"><Upload size={14} strokeWidth={1.75} />Import</Button>
      <Button variant="outline" onclick={() => create("demo")} data-action="new-demo"><Sparkles size={14} strokeWidth={1.75} />New demo</Button>
      <Button variant="primary" onclick={() => create("empty")} data-action="new-empty"><Plus size={14} strokeWidth={1.75} />New</Button>
    </div>
  </header>
  <p class="lede">Each scope is a document kept in this browser. Open one to load the editor.</p>
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if loaded && !rows.length}
    <EmptyState icon={Database} title="No scopes yet" class="empty">
      A scope is a document kept in this browser.
      {#snippet actions()}
        <Button variant="primary" onclick={() => create("empty")}><Plus size={14} strokeWidth={1.75} />New scope</Button>
        <Button variant="outline" onclick={() => create("demo")}><Sparkles size={14} strokeWidth={1.75} />Start from the demo</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="list" aria-label="Stored scopes">
      {#each rows as r (r.id)}
        <li class="row" data-scope={r.id}>
          {#if renaming === r.id}
            <div class="main">
              <!-- svelte-ignore a11y_autofocus -->
              <TextInput
                class="rename"
                aria-label="Scope name"
                autofocus
                bind:value={draft}
                onkeydown={(e: KeyboardEvent) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") renaming = null;
                }}
                onblur={commitRename}
              />
            </div>
          {:else}
            <BitsButton.Root class="main open" title="Open {r.name}" onclick={() => open(r.id)}>
              <span class="name">{r.name}</span>
              <span class="meta">
                {#if r.busy}<span class="busy"><Lock size={12} strokeWidth={1.75} />open in another tab</span> ·{/if}
                {bytes(r.bytes)} · {ago(r.modified)}
              </span>
            </BitsButton.Root>
          {/if}
          <DropdownMenu.Root>
            <DropdownMenu.Trigger class="property-icon large" aria-label="More for {r.name}" title="More">
              <Ellipsis size={16} strokeWidth={1.75} />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class="ui-menu" sideOffset={4} align="end">
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => open(r.id)}><FolderOpen size={14} strokeWidth={1.75} /><span>Open</span></DropdownMenu.Item>
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => startRename(r)}><Pencil size={14} strokeWidth={1.75} /><span>Rename</span></DropdownMenu.Item>
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => act(() => library.duplicate(r.id))}><Copy size={14} strokeWidth={1.75} /><span>Duplicate</span></DropdownMenu.Item>
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => act(() => exportScope(r))}><Download size={14} strokeWidth={1.75} /><span>Export .json</span></DropdownMenu.Item>
                <DropdownMenu.Separator class="ui-menu-separator" />
                <DropdownMenu.Item class="ui-menu-item danger" disabled={r.busy} onSelect={() => (deleting = r)}><Trash size={14} strokeWidth={1.75} /><span>Delete…</span></DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </li>
      {/each}
    </ul>
  {/if}

  {#if storage}
    <p class="storage">{bytes(storage.usage)} used of {bytes(storage.quota)} available to this site</p>
  {/if}
  <input type="file" accept=".json,application/json" bind:this={fileInput} hidden onchange={onFile} />
</main>

<AlertDialog.Root open={!!deleting} onOpenChange={(o) => { if (!o) deleting = null; }}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="ui-dialog-overlay" />
    <AlertDialog.Content class="ui-dialog">
      <AlertDialog.Title class="ui-dialog-title">Delete “{deleting?.name}”?</AlertDialog.Title>
      <div class="ui-dialog-body">
        <AlertDialog.Description class="ui-dialog-description">Its document and history are removed from this browser. This cannot be undone.</AlertDialog.Description>
      </div>
      <div class="ui-dialog-actions">
        <AlertDialog.Cancel class="ui-button is-outline">Cancel</AlertDialog.Cancel>
        <AlertDialog.Action
          class="ui-button is-danger is-outline"
          data-action="confirm-delete"
          onclick={() => {
            const r = deleting;
            deleting = null;
            if (r) void act(() => library.remove(r.id));
          }}>Delete</AlertDialog.Action
        >
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .scopes-page {
    box-sizing: border-box;
    height: 100vh;
    overflow: auto;
    max-width: 760px;
    margin: 0 auto;
    padding: 48px 24px 32px;
    color: var(--fg);
    font-size: var(--text-lg);
  }
  :global(body:has(.scopes-page)) {
    background: var(--bg);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .heading {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .lede {
    margin: 6px 0 20px;
    color: var(--fg-muted);
  }
  .error {
    margin: 0 0 12px;
    color: var(--danger);
  }
  .scopes-page :global(.empty) {
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-lg);
  }
  .list {
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px 0 0;
    border-top: 1px solid var(--border);
  }
  .row:first-child {
    border-top: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row :global(.main) {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 10px 14px;
  }
  .row :global(.open) {
    border: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row :global(.rename) {
    width: 100%;
    font-size: var(--text-lg);
    font-weight: 600;
  }
  .name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-lg);
    font-weight: 600;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--fg-faint);
    font-size: var(--text-sm);
  }
  .busy {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--warning);
  }
  .storage {
    margin: 14px 0 0;
    color: var(--fg-faint);
    font-size: var(--text-sm);
  }
  :global(.ui-menu-item.danger) {
    color: var(--danger);
  }
</style>
