<script lang="ts">
  // The tools, Figma's way: a toolbar floating at the bottom of a 2D
  // composition view. Which tools, in what order, and their keys are context
  // data (plans/specs/contexts.md §2.7): the engine resolves this view's
  // palette and keymap from its focus (its world's notation, the container
  // the selection is open in, editing states) and reports them per panel.
  // By default (`std:free-2d`): Move (V); the shapes, rectangle (R),
  // ellipse (O), table and empty (and port, where a component's main is),
  // behind one button that remembers the last one used; the pen (P); text
  // (T). A tool's icon and label are the shell's, by tool id. The engine
  // reports which tool is active. A view whose palette is empty (a 3D one)
  // has no toolbar.
  import { DropdownMenu } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Circle from "@lucide/svelte/icons/circle";
  import CircleDot from "@lucide/svelte/icons/circle-dot";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import PenTool from "@lucide/svelte/icons/pen-tool";
  import Square from "@lucide/svelte/icons/square";
  import Table from "@lucide/svelte/icons/table";
  import Type from "@lucide/svelte/icons/type";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { Tool } from "../../runtime/document_client.svelte";
  import type { ViewSpec } from "../views";

  let { spec, panel = "" }: { spec: ViewSpec; panel?: string } = $props();

  const doc = controllerManager.docClient;

  const TOOLS: Record<string, { label: string; icon: typeof Square }> = {
    select: { label: "Move", icon: MousePointer2 },
    pen: { label: "Pen", icon: PenTool },
    rect: { label: "Rectangle", icon: Square },
    ellipse: { label: "Ellipse", icon: Circle },
    text: { label: "Text", icon: Type },
    table: { label: "Table", icon: Table },
    port: { label: "Port", icon: CircleDot },
    empty: { label: "Empty", icon: Crosshair },
  };

  const offer = $derived(doc.focus[panel]);
  // The library and the Stats view show their own content, not a world to draw in.
  const shown = $derived(!!offer?.palette && spec.source !== "library" && spec.source !== "stats");
  const slots = $derived((offer?.palette ?? "").split(/\s+/).filter(Boolean).map((slot) => slot.split("|").filter((t) => t in TOOLS)).filter((s) => s.length));
  const keyOf = $derived(Object.fromEntries((offer?.keys ?? "").split(/\s+/).filter(Boolean).map((p) => p.split(":")).map(([k, t]) => [t, k.toUpperCase()])));
  const title = (tool: string) => (keyOf[tool] ? `${TOOLS[tool].label} (${keyOf[tool]})` : TOOLS[tool].label);

  // A shared slot shows the tool last used from it.
  let last: Record<string, string> = $state({});
  $effect(() => {
    for (const slot of slots) if (slot.length > 1 && slot.includes(doc.tool)) last[slot.join("|")] = doc.tool;
  });
  const current = (slot: string[]) => (slot.includes(last[slot.join("|")]) ? last[slot.join("|")] : slot[0]);
  let menu: Record<string, boolean> = $state({});

  function pick(tool: string, slot?: string[]) {
    if (slot) menu[slot.join("|")] = false;
    doc.setTool(tool as Tool);
  }
</script>


{#if shown}
  <div class="toolbar canvas-ui" role="toolbar" aria-label="Tools">
    {#each slots as slot (slot.join("|"))}
      {#if slot.length === 1}
        {@const t = TOOLS[slot[0]]}
        <button class="tool" class:active={doc.tool === slot[0]} title={title(slot[0])} aria-label={t.label} onclick={() => pick(slot[0])}>
          <t.icon size={18} strokeWidth={1.6} />
        </button>
      {:else}
        {@const id = current(slot)}
        {@const t = TOOLS[id]}
        <div class="group">
          <button class="tool" class:active={doc.tool === id} title={title(id)} aria-label={t.label} onclick={() => pick(id)}>
            <t.icon size={18} strokeWidth={1.6} />
          </button>
          <DropdownMenu.Root bind:open={() => menu[slot.join("|")] ?? false, (v) => (menu[slot.join("|")] = v)}>
            <DropdownMenu.Trigger class="more" title="Shape tools" aria-label="Shape tools">
              <ChevronDown size={12} strokeWidth={2} />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class="ui-menu" side="top" sideOffset={8} align="start">
                {#each slot as s (s)}
                  {@const m = TOOLS[s]}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => pick(s, slot)}>
                    <span class="check">{#if doc.tool === s}<Check size={14} strokeWidth={2} />{/if}</span>
                    <m.icon size={15} strokeWidth={1.75} />
                    <span>{m.label}</span>
                    <span class="ui-hint">{keyOf[s] ?? ""}</span>
                  </DropdownMenu.Item>
                {/each}
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .toolbar {
    position: absolute;
    left: 50%;
    bottom: 16px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    background: var(--bg);
    box-shadow: var(--menu-shadow);
    user-select: none;
  }
  .group {
    display: flex;
    align-items: center;
  }
  .tool {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--fg);
    cursor: pointer;
  }
  .tool:hover {
    background: var(--hover);
  }
  .tool.active {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .group :global(.more) {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 36px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }
  .group :global(.more:hover) {
    background: var(--hover);
    color: var(--fg);
  }
  .check {
    display: flex;
    width: 14px;
  }
</style>
