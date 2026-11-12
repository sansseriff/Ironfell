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
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";
  import AudioWaveform from "@lucide/svelte/icons/audio-waveform";
  import BatteryMedium from "@lucide/svelte/icons/battery-medium";
  import Cable from "@lucide/svelte/icons/cable";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Circle from "@lucide/svelte/icons/circle";
  import CircleDot from "@lucide/svelte/icons/circle-dot";
  import CircleArrowUp from "@lucide/svelte/icons/circle-arrow-up";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import Gauge from "@lucide/svelte/icons/gauge";
  import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
  import Pause from "@lucide/svelte/icons/pause";
  import PenTool from "@lucide/svelte/icons/pen-tool";
  import RectangleHorizontal from "@lucide/svelte/icons/rectangle-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import Table from "@lucide/svelte/icons/table";
  import Type from "@lucide/svelte/icons/type";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { Tool } from "../../runtime/document_client.svelte";
  import type { ViewSpec } from "../views";
  import TimelinePanel from "./TimelinePanel.svelte";
  import X from "@lucide/svelte/icons/x";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import { onDestroy } from "svelte";
  import { machines } from "../../lib/machines.svelte";

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
    // The schematic's (schematic.md §3): the wire, and placing each
    // standard part (`place:<key>`).
    wire: { label: "Wire", icon: Cable },
    "place:resistor": { label: "Resistor", icon: RectangleHorizontal },
    "place:capacitor": { label: "Capacitor", icon: Pause },
    "place:inductor": { label: "Inductor", icon: AudioWaveform },
    "place:vsource": { label: "Voltage source", icon: BatteryMedium },
    "place:isource": { label: "Current source", icon: CircleArrowUp },
    "place:ground": { label: "Ground", icon: ArrowDownToLine },
    "place:probe": { label: "Probe", icon: Gauge },
  };

  const offer = $derived(doc.focus[panel]);
  // The library and the timeline show their own content,
  // not a world to draw in. The timeline has its DOM column instead
  // (TimelinePanel.svelte, timeline.md §12.3).
  const timeline = $derived(spec.source === "timeline");
  const shown = $derived(!!offer?.palette && spec.source !== "library" && !timeline);
  const slots = $derived((offer?.palette ?? "").split(/\s+/).filter(Boolean).map((slot) => slot.split("|").filter((t) => t in TOOLS)).filter((s) => s.length));
  // `key:tool`, where a tool may hold colons itself (`r:place:resistor`).
  const keyOf = $derived(Object.fromEntries((offer?.keys ?? "").split(/\s+/).filter(Boolean).map((p) => [p.slice(p.indexOf(":") + 1), p.slice(0, p.indexOf(":")).toUpperCase()])));
  const title = (tool: string) => (keyOf[tool] ? `${TOOLS[tool].label} (${keyOf[tool]})` : TOOLS[tool].label);

  // A shared slot shows the tool last used from it.
  let last: Record<string, string> = $state({});
  $effect(() => {
    for (const slot of slots) if (slot.length > 1 && slot.includes(doc.tool)) last[slot.join("|")] = doc.tool;
  });
  const current = (slot: string[]) => (slot.includes(last[slot.join("|")]) ? last[slot.join("|")] : slot[0]);
  let menu: Record<string, boolean> = $state({});

  // A state previewed on a main says so on the canvas, loudly, with a way
  // out (machines.md §6): what you see is not the component's resting look.
  const stopMachines = machines.use();
  onDestroy(stopMachines);
  const pin = $derived(machines.pinned);

  function pick(tool: string, slot?: string[]) {
    if (slot) menu[slot.join("|")] = false;
    doc.setTool(tool as Tool);
  }
</script>

{#if timeline}
  <TimelinePanel {panel} />
{/if}

{#if shown && pin}
  <!-- Recording into a state (orange) and previewing a transition, which
       records nothing (blue), never look alike. -->
  <div class="previewing canvas-ui" class:scrub={!!pin.scrub} role="status">
    <span class="dot"></span>
    {#if pin.scrub}
      <span>Previewing <strong>{pin.scrub.from} → {pin.scrub.to}</strong> at {Math.round(pin.scrub.progress * 100)}%<span class="of"> · nothing records</span></span>
    {:else}
      <span>Editing <strong>{pin.state}</strong>{#if pin.name}<span class="of"> · {pin.name}</span>{/if}<span class="of"> · edits go into this state</span></span>
    {/if}
    <IconButton icon={X} class="stop" onclick={() => machines.pin(pin.def, pin.machine, null)} title={pin.scrub ? "Stop previewing" : "Stop editing this state: the component shows at rest"} label={pin.scrub ? "Stop previewing" : "Stop editing this state"} />
  </div>
{/if}

{#if shown}
  <div class="toolbar canvas-ui" role="toolbar" aria-label="Tools">
    {#each slots as slot (slot.join("|"))}
      {#if slot.length === 1}
        {@const t = TOOLS[slot[0]]}
        <button class="ui-icon-button is-tool tool" aria-pressed={doc.tool === slot[0]} title={title(slot[0])} aria-label={t.label} onclick={() => pick(slot[0])}>
          <t.icon size={18} strokeWidth={1.75} />
        </button>
      {:else}
        {@const id = current(slot)}
        {@const t = TOOLS[id]}
        <div class="group">
          <button class="ui-icon-button is-tool tool" aria-pressed={doc.tool === id} title={title(id)} aria-label={t.label} onclick={() => pick(id)}>
            <t.icon size={18} strokeWidth={1.75} />
          </button>
          <DropdownMenu.Root bind:open={() => menu[slot.join("|")] ?? false, (v) => (menu[slot.join("|")] = v)}>
            <DropdownMenu.Trigger class="ui-icon-button is-tool-menu" title="Shape tools" aria-label="Shape tools">
              <ChevronDown size={12} strokeWidth={1.75} />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class="ui-menu" side="top" sideOffset={8} align="start">
                {#each slot as s (s)}
                  {@const m = TOOLS[s]}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => pick(s, slot)}>
                    <span class="ui-menu-check">{#if doc.tool === s}<Check size={14} strokeWidth={1.75} />{/if}</span>
                    <m.icon size={14} strokeWidth={1.75} />
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
  /* Recording into a state is the record colour; previewing a transition,
     which records nothing, the accent. Text and the dot are a dark shade of
     the pill's own colour. */
  .previewing { --pill: var(--record); --pill-fg: color-mix(in srgb, var(--pill) 18%, black); position: absolute; top: 10px; left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 8px; height: var(--control-h); padding: 0 2px 0 12px; border-radius: calc(var(--control-h) / 2); background: var(--pill); color: var(--pill-fg); font-size: var(--text-md); box-shadow: var(--menu-shadow); pointer-events: auto; z-index: 4; white-space: nowrap; }
  .previewing.scrub { --pill: var(--accent); --pill-fg: var(--accent-fg); }
  .previewing .dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
  .previewing .of { opacity: 0.75; }
  .previewing :global(.stop) { border-radius: 50%; color: inherit; background: color-mix(in srgb, currentColor 12%, transparent); }
  .previewing :global(.stop:hover:not(:disabled)) { color: inherit; background: color-mix(in srgb, currentColor 22%, transparent); }
  /* The floating tool palette: the tool buttons (`.ui-icon-button.is-tool`,
     36 px, larger than a panel's) on a raised bar with the composer's radius. */
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
    border-radius: var(--radius-xl);
    background: var(--raised);
    box-shadow: var(--menu-shadow);
    user-select: none;
  }
  .group {
    display: flex;
    align-items: center;
  }
</style>
