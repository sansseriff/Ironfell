<script lang="ts">
  // The scope menu (plans/specs/scopes.md §6), at the left of the top-left
  // tab group's header: the open scope's name, editable in place; New,
  // Duplicate, Import, Export; the stored scopes to open or delete; and the
  // storage used. Each action is a `Scopes` call (runtime/scopes.ts), also
  // reachable from the driver as `__iron.scopes.*`.
  import type { DockviewApi, DockviewGroupPanel } from "dockview-core";
  import { onMount } from "svelte";
  import { Button as BitsButton, Popover } from "bits-ui";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Trash from "@lucide/svelte/icons/trash-2";
  import Database from "@lucide/svelte/icons/database";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import { scopesUrl } from "../route";
  import { controllerManager } from "../controller-manager.svelte";
  import Button from "../lib/ui/Button.svelte";
  import TextInput from "../lib/ui/TextInput.svelte";
  import type { ScopeMeta } from "../runtime/scope_storage";

  import { dockState } from "./dock_state.svelte";
  let { api, group }: { api: DockviewApi; group: DockviewGroupPanel } = $props();

  const doc = controllerManager.docClient;
  let open = $state(false);
  let shown = $state(false);
  let scopes: (ScopeMeta & { bytes: number })[] = $state([]);
  let storage: { usage: number; quota: number } | null = $state(null);
  let name = $state("");
  /** The scope the name field was filled from: a rename applies only to it,
   *  so a blur after a switch does not rename the scope switched to. */
  let naming: string | null = null;
  let confirming: string | null = $state(null);
  let busy = $state(false);
  let error = $state("");
  let fileInput: HTMLInputElement | undefined = $state();

  // One per window: in the leftmost tab bar along the top that is showing
  // (a group with its tab bar hidden passes it on, panel-primitives.md).
  function place() {
    const top = api.groups
      .filter((g) => !g.header.hidden && g.element.getBoundingClientRect().top < 2)
      .sort((a, b) => a.element.getBoundingClientRect().left - b.element.getBoundingClientRect().left);
    shown = top[0]?.id === group.id;
  }
  onMount(() => {
    place();
    const d = api.onDidLayoutChange(place);
    return () => d.dispose();
  });
  $effect(() => {
    void dockState.hidden;
    place();
  });

  const current = $derived(doc.scope.current);

  async function refresh() {
    if (!doc.scopes) return;
    scopes = await doc.scopes.list();
    storage = await doc.scopes.estimate();
  }

  async function opened(now: boolean) {
    confirming = null;
    error = "";
    name = current?.name ?? "";
    naming = now ? (current?.id ?? null) : null;
    if (now) await refresh();
  }

  /** Run a menu action: busy while it runs, its error shown in the menu. */
  async function act(work: () => Promise<unknown>, close = true) {
    busy = true;
    error = "";
    try {
      await work();
      if (close) open = false;
      else await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
      confirming = null;
    }
  }

  function commitName() {
    if (!doc.scopes || !current || current.id !== naming) return;
    if (name.trim() && name.trim() !== current.name) void act(() => doc.scopes!.rename(name), false);
  }

  async function onFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (file) await act(() => doc.openFile(file));
  }

  function remove(id: string) {
    if (confirming !== id) {
      confirming = id;
      return;
    }
    // Deleting the open scope switches to another: close, as other switches do.
    void act(() => doc.scopes!.remove(id), id !== current?.id ? false : true);
  }

  const bytes = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);
  function ago(ms: number) {
    const s = (Date.now() - ms) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return new Date(ms).toLocaleDateString();
  }
</script>

<div class="scope" class:hidden={!shown}>
  {#if shown}
    <Popover.Root bind:open onOpenChange={opened}>
      <Popover.Trigger class="ui-quiet-trigger scope-trigger" title="Scopes: open, new, import, export" data-scope-menu>
        <Database size={14} strokeWidth={1.75} />
        <span class="scope-name">{doc.scopes ? (current?.name ?? "…") : "Unsaved demo"}</span>
        {#if doc.scope.readOnly}<span class="ui-badge">read-only</span>{/if}
        <ChevronDown size={12} strokeWidth={1.75} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="property-popover scope-popover" side="bottom" align="start" sideOffset={4} aria-label="Scopes">
          {#if !doc.scopes}
            <div class="body">
              <p>This page is not saved: it opened with <code>?demo</code> (or as a benchmark, or without browser storage).</p>
              <p class="hint">Remove <code>demo</code> from the address to keep work between reloads.</p>
            </div>
          {:else}
            <div class="body">
              <Button
                variant="outline"
                small
                class="all-scopes"
                data-action="all-scopes"
                disabled={busy}
                title="Close this scope and go to the scopes page"
                onclick={() => act(async () => {
                  await doc.scopes!.close();
                  location.href = scopesUrl();
                })}><LayoutGrid size={14} strokeWidth={1.75} />All scopes</Button
              >
              <TextInput
                class="name"
                aria-label="Scope name"
                bind:value={name}
                disabled={!current || doc.scope.readOnly}
                onkeydown={(e: KeyboardEvent) => {
                  if (e.key === "Enter") commitName();
                  if (e.key === "Escape") name = current?.name ?? "";
                }}
                onblur={commitName}
              />
              {#if doc.scope.notice}
                <p class="notice">{doc.scope.notice}</p>
              {/if}
              {#if doc.scope.readOnly}
                <Button variant="primary" class="take-over" disabled={busy} onclick={() => act(() => doc.scopes!.takeOver())}>Open here instead</Button>
              {/if}
              <div class="actions">
                <Button variant="outline" small disabled={busy} onclick={() => act(() => doc.scopes!.create("empty"))} data-action="new-empty">New</Button>
                <Button variant="outline" small disabled={busy} onclick={() => act(() => doc.scopes!.create("demo"))} data-action="new-demo">New demo</Button>
                <Button variant="outline" small disabled={busy || !current} onclick={() => act(() => doc.scopes!.duplicate())} data-action="duplicate">Duplicate</Button>
                <Button variant="outline" small disabled={busy} onclick={() => fileInput?.click()} data-action="import">Import…</Button>
                <Button variant="outline" small disabled={busy || !current} onclick={() => act(() => doc.download())} data-action="export">Export</Button>
              </div>
              <input type="file" accept=".json,application/json" bind:this={fileInput} hidden onchange={onFile} />
              {#if error}<p class="error">{error}</p>{/if}
            </div>
            <div class="list" role="list" aria-label="Stored scopes">
              {#each scopes as s (s.id)}
                <div class="item" role="listitem" class:current={s.id === current?.id}>
                  <BitsButton.Root class="open" disabled={busy || s.id === current?.id} onclick={() => act(() => doc.scopes!.open(s.id))} title={s.id}>
                    <span class="item-name">{s.name}</span>
                    <span class="meta">{bytes(s.bytes)} · {ago(s.modified)}</span>
                  </BitsButton.Root>
                  <!-- One element both ways, so focus stays on it for the second press. -->
                  <BitsButton.Root
                    class="property-icon delete {confirming === s.id ? 'confirming' : ''}"
                    disabled={busy}
                    aria-label={confirming === s.id ? `Delete ${s.name}: cannot be undone` : `Delete ${s.name}`}
                    title={confirming === s.id ? "Click again to delete; this cannot be undone" : "Delete"}
                    onclick={() => remove(s.id)}
                  >
                    {#if confirming === s.id}Delete?{:else}<Trash size={14} strokeWidth={1.75} />{/if}
                  </BitsButton.Root>
                </div>
              {/each}
            </div>
            {#if storage}
              <p class="storage">{bytes(storage.usage)} used of {bytes(storage.quota)} available to this site</p>
            {/if}
          {/if}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  {/if}
</div>

<style>
  .scope {
    flex: none;
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 8px;
    pointer-events: auto;
  }
  .scope.hidden {
    display: none;
  }
  .scope :global(.scope-trigger) {
    max-width: 200px;
    flex-shrink: 0;
    font-weight: 600;
  }
  .scope :global(.scope-trigger > svg:first-child) {
    flex: none;
    color: var(--fg-muted);
  }
  .scope-name {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(.scope-popover) {
    width: 320px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: var(--panel-pad);
    border-bottom: 1px solid var(--border);
  }
  .body :global(.name) {
    font-size: var(--text-lg);
    font-weight: 600;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .notice {
    margin: 0;
    color: var(--fg-muted);
  }
  .hint {
    color: var(--fg-muted);
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .body :global(.all-scopes),
  .body :global(.take-over) {
    align-self: flex-start;
  }
  .list {
    max-height: 260px;
    overflow: auto;
    padding: 4px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 4px;
    border-radius: var(--radius);
  }
  .item:hover {
    background: var(--hover);
  }
  .item.current {
    background: var(--accent-soft);
  }
  .item :global(.open) {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 4px 8px;
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item :global(.open:disabled) {
    cursor: default;
  }
  .item-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .meta {
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }
  .item :global(.delete) {
    margin-right: 4px;
  }
  .item :global(.delete:hover) {
    color: var(--danger);
  }
  .item :global(.delete.confirming) {
    width: auto;
    padding: 0 8px;
    background: var(--danger);
    color: var(--accent-fg);
    font-size: var(--text-sm);
  }
  .storage {
    margin: 0;
    padding: 8px var(--panel-pad) 10px;
    border-top: 1px solid var(--border);
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }
</style>
