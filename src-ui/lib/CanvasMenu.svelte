<script lang="ts">
  // The canvas's right-click menu (plans/specs/contexts.md §4.2): the items
  // are the view's `menu.canvas`, resolved by the engine after the click has
  // selected what it was on (EngineEvent `menu`). Nothing here names a
  // command: each item is drawn from what the engine sent and runs by id.
  // A Bits dropdown anchored at the click, since its items arrive after the
  // event (a context menu trigger would open before they are known).
  import { DropdownMenu } from "bits-ui";
  import type { Component } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import { commandFailure } from "../runtime/commands.svelte";
  import { chordLabel } from "../runtime/chords";
  import Copy from "@lucide/svelte/icons/copy";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import FileCode from "@lucide/svelte/icons/file-code";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import BringToFront from "@lucide/svelte/icons/bring-to-front";
  import SendToBack from "@lucide/svelte/icons/send-to-back";
  import SquareDashed from "@lucide/svelte/icons/square-dashed";
  import Rows3 from "@lucide/svelte/icons/rows-3";
  import ComponentIcon from "@lucide/svelte/icons/component";
  import Scissors from "@lucide/svelte/icons/scissors";
  import CopyPlus from "@lucide/svelte/icons/copy-plus";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import LassoSelect from "@lucide/svelte/icons/lasso-select";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Scan from "@lucide/svelte/icons/scan";
  import Scaling from "@lucide/svelte/icons/scaling";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";

  // The registry's icon names (commands.rs) this menu can draw; another
  // command's item has none.
  const ICONS: Record<string, Component<{ size?: number; strokeWidth?: number }>> = {
    copy: Copy, "clipboard-paste": ClipboardPaste, "file-code": FileCode, sparkles: Sparkles, "pen-line": PenLine,
    "bring-to-front": BringToFront, "send-to-back": SendToBack, "square-dashed": SquareDashed, "rows-3": Rows3,
    component: ComponentIcon, scissors: Scissors, "copy-plus": CopyPlus, "undo-2": Undo2, "redo-2": Redo2,
    "lasso-select": LassoSelect, maximize: Maximize, scan: Scan, scaling: Scaling, "zoom-in": ZoomIn, "zoom-out": ZoomOut,
  };

  const doc = controllerManager.docClient;
  let open = $state(false);
  // Where the menu opens: a zero-size box at the click.
  let anchor = $state({ x: 0, y: 0 });
  // The click in canvas px, for Paste here.
  let canvasAt: { x: number; y: number } | null = null;
  const point = { getBoundingClientRect: () => DOMRect.fromRect({ x: anchor.x, y: anchor.y, width: 0, height: 0 }) };
  // Why the last command run from the menu could not, shown at the click.
  let ranAt = 0;
  let note = $state<{ text: string; x: number; y: number } | null>(null);
  let noteTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const m = doc.menu;
    if (!m) return;
    anchor = m.client;
    canvasAt = m.canvas;
    open = true;
  });
  $effect(() => {
    const f = commandFailure.current;
    if (!f || f.at < ranAt || f.at - ranAt > 3000) return;
    note = { text: f.reason, x: anchor.x, y: anchor.y };
    clearTimeout(noteTimer);
    noteTimer = setTimeout(() => (note = null), 2500);
  });

  function run(id: string) {
    ranAt = performance.now();
    void doc.runCommand(id, canvasAt);
  }
</script>

<DropdownMenu.Root bind:open onOpenChange={(o) => { if (!o) doc.menu = null; }}>
  <!-- Bits wants a trigger; the click on the canvas is the real one. -->
  <DropdownMenu.Trigger class="canvas-menu-anchor" tabindex={-1} aria-hidden="true" />
  <DropdownMenu.Portal>
    <DropdownMenu.Content class="ui-menu canvas-menu" customAnchor={point} side="bottom" align="start" sideOffset={2} data-canvas-menu>
      {#each doc.menu?.items ?? [] as item, i (item ? item.id : `divider-${i}`)}
        {#if item}
          {@const Icon = ICONS[item.icon]}
          <DropdownMenu.Item class="ui-menu-item" disabled={!item.enabled} title={item.enabled ? undefined : item.reason} onSelect={() => run(item.id)} data-command={item.id}>
            <span class="ui-menu-check">{#if Icon}<Icon size={14} strokeWidth={1.75} />{/if}</span>
            <span>{item.label}</span>
            {#if item.chord}<span class="ui-hint">{chordLabel(item.chord)}</span>{/if}
          </DropdownMenu.Item>
        {:else}
          <div class="ui-menu-separator"></div>
        {/if}
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

{#if note}
  <div class="ui-tooltip canvas-menu-note" style:left="{note.x}px" style:top="{note.y}px" role="status">{note.text}</div>
{/if}

<style>
  :global(.canvas-menu-anchor) { position: fixed; left: 0; top: 0; width: 0; height: 0; opacity: 0; pointer-events: none !important; border: 0; padding: 0; background: none; }
  :global(.canvas-menu) { min-width: 220px; }
  :global(.canvas-menu .ui-menu-check) { color: var(--fg-muted); }
  :global(.canvas-menu .ui-menu-item[data-highlighted] .ui-menu-check) { color: inherit; }
  .canvas-menu-note { position: fixed; transform: translate(8px, 8px); }
</style>
