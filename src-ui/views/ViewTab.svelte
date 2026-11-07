<script lang="ts">
  // One tab in the dock, VS Code's way: the view's icon, its title, and a
  // close button shown on hover and on the active tab. Middle-click closes.
  // The rounded shape is the dock's CSS (Workspace.svelte). On a view of one
  // of the document's compositions, double-clicking the title renames the
  // composition, and the context menu renames or deletes it (each one
  // undoable transaction); the title follows its name (workspace.ts).
  import { onDestroy, tick, type Component } from "svelte";
  import { ContextMenu } from "bits-ui";
  import type { DockviewPanelApi } from "dockview-core";
  import Shapes from "@lucide/svelte/icons/shapes";
  import FileCode from "@lucide/svelte/icons/file-code";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Bot from "@lucide/svelte/icons/bot";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Settings from "@lucide/svelte/icons/settings";
  import Timer from "@lucide/svelte/icons/timer";
  import Layers from "@lucide/svelte/icons/layers";
  import X from "@lucide/svelte/icons/x";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash from "@lucide/svelte/icons/trash-2";
  import type { Notation, ViewSpec } from "./views";
  import { controllerManager } from "../controller-manager.svelte";

  let { api, notation, spec }: { api: DockviewPanelApi; notation: Notation | undefined; spec?: ViewSpec } = $props();

  const doc = controllerManager.docClient;
  /** The document composition this tab shows, by id; none for the app's views. */
  const composition = $derived(
    spec?.notation === "composition" && spec.source.startsWith("#") ? doc.compositions.find((c) => c.id === spec.source && !c.session) : undefined,
  );

  let editing = $state(false);
  let draft = $state("");
  let input: HTMLInputElement | undefined = $state();

  async function beginRename() {
    if (!composition) return;
    draft = composition.name ?? "";
    editing = true;
    await tick();
    input?.select();
  }

  async function commitRename() {
    if (!editing || !composition) return;
    editing = false;
    const name = draft.trim();
    if (!name || name === composition.name) return;
    if (doc.compositions.some((c) => c.name === name)) {
      console.warn(`a composition named "${name}" already exists`);
      return;
    }
    const r = await doc.command<{ ok: boolean; error?: string }>({ op: "author", verb: "rename", args: JSON.stringify({ node: composition.id, name }) });
    if (!r.ok) console.warn("rename refused:", r.error);
  }

  async function remove() {
    if (!composition) return;
    const r = await doc.command<{ ok: boolean; error?: string }>({ op: "author", verb: "delete_composition", args: JSON.stringify({ node: composition.id }) });
    if (r.ok) api.close();
    else console.warn("delete refused:", r.error);
  }

  const ICONS: Record<Notation, Component> = {
    composition: Shapes,
    layers: Layers,
    markup: FileCode,
    selection: SlidersHorizontal,
    conversation: Bot,
    narrative: BookOpen,
    settings: Settings,
    latency: Timer,
  };
  const Icon = $derived(notation ? ICONS[notation] : Shapes);

  let title = $state(api.title ?? "");
  const sub = api.onDidTitleChange((e) => (title = e.title));
  onDestroy(() => sub.dispose());
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger>
    {#snippet child({ props })}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        {...props}
        class="view-tab"
        onauxclick={(e) => {
          if (e.button === 1) {
            e.preventDefault();
            api.close();
          }
        }}
      >
        <Icon size={14} strokeWidth={1.75} class="icon" />
        {#if editing}
          <input
            class="rename ui-input"
            bind:this={input}
            bind:value={draft}
            aria-label="Composition name"
            spellcheck="false"
            onpointerdown={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") void commitRename();
              if (e.key === "Escape") editing = false;
            }}
            onblur={() => void commitRename()}
          />
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="title" ondblclick={() => void beginRename()} title={composition ? "Double-click to rename" : undefined}>{title}</span>
        {/if}
        <button
          class="close"
          title="Close"
          aria-label="Close {title}"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => {
            e.stopPropagation();
            api.close();
          }}
        >
          <X size={13} strokeWidth={1.75} />
        </button>
      </div>
    {/snippet}
  </ContextMenu.Trigger>
  <ContextMenu.Portal>
    <ContextMenu.Content class="ui-menu">
      {#if composition}
        <ContextMenu.Item class="ui-menu-item" onSelect={() => void beginRename()}>
          <Pencil size={14} strokeWidth={1.75} /> Rename composition
        </ContextMenu.Item>
        <ContextMenu.Item class="ui-menu-item danger" onSelect={() => void remove()}>
          <Trash size={14} strokeWidth={1.75} /> Delete composition
        </ContextMenu.Item>
        <ContextMenu.Separator class="ui-menu-separator" />
      {/if}
      <ContextMenu.Item class="ui-menu-item" onSelect={() => api.close()}>
        <X size={14} strokeWidth={1.75} /> Close tab
      </ContextMenu.Item>
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>

<style>
  .view-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 6px 0 12px;
    white-space: nowrap;
  }
  .view-tab :global(.icon) {
    flex-shrink: 0;
    opacity: 0.85;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rename {
    width: 120px;
    height: 22px;
    padding: 0 6px;
  }
  :global(.ui-menu-item.danger) {
    color: var(--danger);
  }
  .close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    margin-left: 2px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    visibility: hidden;
  }
  .close:hover {
    background: var(--hover);
    color: var(--fg);
  }
  :global(.dv-tab.dv-active-tab) .close,
  .view-tab:hover .close {
    visibility: visible;
  }
</style>
