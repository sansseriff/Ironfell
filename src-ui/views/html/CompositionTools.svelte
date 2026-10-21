<script lang="ts">
  // The tools, Figma's way: a toolbar floating at the bottom of a 2D
  // composition view. Select (V); the shapes, rectangle (R) and ellipse (O),
  // and the component tools, port and empty (component-authoring.md §2),
  // behind one button that remembers the last one used; the pen (P); text
  // (T). The
  // engine reports which tool is active. A 3D composition has no toolbar.
  import { DropdownMenu } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Circle from "@lucide/svelte/icons/circle";
  import CircleDot from "@lucide/svelte/icons/circle-dot";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import PenTool from "@lucide/svelte/icons/pen-tool";
  import Square from "@lucide/svelte/icons/square";
  import Type from "@lucide/svelte/icons/type";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { Tool } from "../../runtime/document_client.svelte";
  import type { ViewSpec } from "../views";
  import CompositionSettings from "./CompositionSettings.svelte";

  let { spec }: { spec: ViewSpec } = $props();

  const doc = controllerManager.docClient;

  // Whether this view's composition is 2D, re-read as the document changes.
  let twoD = $state(false);
  $effect(() => {
    void doc.version;
    const source = spec.source;
    doc
      .call("compositions", [])
      .then((list: { id: string; name: string | null; dims: string }[]) => {
        const c = list.find((c) => c.id === source || c.name === source);
        // The Library tab is browsed and Stats is read, not drawn in.
        twoD = c?.dims === "2d" && source !== "library" && source !== "stats";
      })
      .catch(() => (twoD = false));
  });

  const SHAPES = [
    { id: "rect", key: "R", label: "Rectangle", icon: Square },
    { id: "ellipse", key: "O", label: "Ellipse", icon: Circle },
    { id: "port", key: "", label: "Port (on a component's main)", icon: CircleDot },
    { id: "empty", key: "", label: "Empty", icon: Crosshair },
  ] as const;
  let lastShape: (typeof SHAPES)[number]["id"] = $state("rect");
  $effect(() => {
    if (SHAPES.some((s) => s.id === doc.tool)) lastShape = doc.tool as (typeof SHAPES)[number]["id"];
  });
  const shape = $derived(SHAPES.find((s) => s.id === lastShape)!);
  let menu = $state(false);

  function pick(tool: Tool) {
    menu = false;
    doc.setTool(tool);
  }
</script>

<CompositionSettings {spec} />

{#if twoD}
  <div class="toolbar canvas-ui" role="toolbar" aria-label="Tools">
    <button class="tool" class:active={doc.tool === "select"} title="Move (V)" aria-label="Move" onclick={() => pick("select")}>
      <MousePointer2 size={18} strokeWidth={1.6} />
    </button>
    <div class="group">
      <button class="tool" class:active={doc.tool === shape.id} title="{shape.label} ({shape.key})" aria-label={shape.label} onclick={() => pick(shape.id)}>
        <shape.icon size={18} strokeWidth={1.6} />
      </button>
      <DropdownMenu.Root bind:open={menu}>
        <DropdownMenu.Trigger class="more" title="Shape tools" aria-label="Shape tools">
          <ChevronDown size={12} strokeWidth={2} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class="ui-menu" side="top" sideOffset={8} align="start">
            {#each SHAPES as s (s.id)}
              <DropdownMenu.Item class="ui-menu-item" onSelect={() => pick(s.id)}>
                <span class="check">{#if doc.tool === s.id}<Check size={14} strokeWidth={2} />{/if}</span>
                <s.icon size={15} strokeWidth={1.75} />
                <span>{s.label}</span>
                <span class="ui-hint">{s.key}</span>
              </DropdownMenu.Item>
            {/each}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
    <button class="tool" class:active={doc.tool === "pen"} title="Pen (P)" aria-label="Pen" onclick={() => pick("pen")}>
      <PenTool size={18} strokeWidth={1.6} />
    </button>
    <button class="tool" class:active={doc.tool === "text"} title="Text (T)" aria-label="Text" onclick={() => pick("text")}>
      <Type size={18} strokeWidth={1.6} />
    </button>
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
