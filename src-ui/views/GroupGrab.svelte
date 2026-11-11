<script lang="ts">
  // A group's grab handle while its tab bar is hidden (panel-primitives.md):
  // a small pill at the top centre of its view. Click: the group's views to
  // switch, Show tab bar, Close. Drag: dockview's own drag of this view (or,
  // with Alt, of the whole group), started from its hidden tab, so its drop
  // zones, splits and moves work as from a tab.
  import type { DockviewPanelApi } from "dockview-core";
  import { DropdownMenu } from "bits-ui";
  import GripHorizontal from "@lucide/svelte/icons/grip-horizontal";
  import { dockApi, setTabBarHidden } from "./workspace";

  let { api }: { api: DockviewPanelApi } = $props();
  let open = $state(false);
  // The pill paints only while the pointer is in the group's top strip (a
  // panel header's height), so a bare group shows no stray nub at rest.
  let near = $state(false);
  $effect(() => {
    const el = (api.group as unknown as { element: HTMLElement }).element;
    const strip = parseFloat(getComputedStyle(el).getPropertyValue("--panel-header-h")) || 36;
    // On the window: over a canvas view the pointer's events go to the canvas.
    const move = (e: PointerEvent) => {
      const r = el.getBoundingClientRect();
      const inside = e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.top + strip;
      if (inside !== near) near = inside;
    };
    window.addEventListener("pointermove", move, { passive: true });
    return () => window.removeEventListener("pointermove", move);
  });
  const views = () => api.group.panels.map((p) => ({ id: p.id, title: p.title ?? p.id, active: p.id === api.id }));

  // A press hands the drag to dockview: its own pointer drag starts from a
  // tab (or, for the whole group, the header's empty part), so the same
  // event goes to the group's hidden active tab and dockview takes it from
  // there: its threshold, ghost, drop zones and move. A press that does not
  // move is a click, which opens the menu.
  function press(e: PointerEvent) {
    if (e.button !== 0) return;
    const el = (api.group as unknown as { element: HTMLElement }).element;
    const target = e.altKey ? el.querySelector(".dv-void-container") : el.querySelector(".dv-tab.dv-active-tab");
    if (!target) return;
    target.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true, composed: true, pointerId: e.pointerId, pointerType: e.pointerType, isPrimary: true, clientX: e.clientX, clientY: e.clientY, screenX: e.screenX, screenY: e.screenY, button: 0, buttons: 1, altKey: e.altKey }));
  }
</script>

<div class="grab canvas-ui" class:near>
  <DropdownMenu.Root bind:open>
    <!-- The menu's anchor: Bits' trigger opens on pointerdown and cancels it,
         which would stop the browser's drag, so the pill is a plain button
         over it that opens the menu on click and hands a drag to dockview. -->
    <DropdownMenu.Trigger class="grab-anchor" tabindex={-1} aria-hidden="true" />
    <button class="grab-pill" class:open onpointerdown={press} onclick={() => (open = !open)}
      aria-haspopup="menu" aria-expanded={open} aria-label="{api.title}: views, show the tab bar, or drag to move" title="{api.title} · click for views, drag to move (Alt: the whole group)">
      <GripHorizontal size={14} strokeWidth={2} />
    </button>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu" align="center" sideOffset={4}>
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Views here</DropdownMenu.GroupHeading>
          {#each views() as v (v.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => dockApi()?.getPanel(v.id)?.api.setActive()}>{v.title}{#if v.active}<span class="ui-hint">showing</span>{/if}</DropdownMenu.Item>
          {/each}
        </DropdownMenu.Group>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Item class="ui-menu-item" onSelect={() => setTabBarHidden(api.group.id, false)}>Show tab bar</DropdownMenu.Item>
        <DropdownMenu.Item class="ui-menu-item" onSelect={() => api.close()}>Close {api.title}</DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
</div>

<style>
  /* Collapsed, the hit area is a 10 px strip at the view's top edge, clear
     of a panel header's controls (centred in its 36 px row); hovered, open
     or focused it grows down to the full pill. Growing never moves the
     target away from the pointer, so it cannot flicker. */
  .grab { position: absolute; top: 0; left: 50%; width: 40px; height: 10px; transform: translateX(-50%); z-index: 5; pointer-events: auto; }
  .grab:hover, .grab:has(.grab-pill.open), .grab:has(.grab-pill:focus-visible) { height: 22px; }
  /* Workspace's canvas-ui pointer rule otherwise re-enables this invisible
     trigger over the pill, alternately taking hover and changing the cursor. */
  :global(.grab-anchor) { position: absolute; inset: 0; opacity: 0; pointer-events: none !important; border: 0; padding: 0; background: none; }
  /* The full-size button remains the hit area. Only its painted pill grows,
     so hovering near the collapsed edge cannot move the pointer target. */
  .grab-pill { position: relative; display: grid; place-items: center; width: 40px; height: 100%; padding: 0; border: 0; background: transparent; color: var(--fg-muted); cursor: grab; }
  .grab-pill::before { content: ''; position: absolute; top: 0; left: 0; right: 0; height: 18px; border: 1px solid var(--border); border-top: 0; border-radius: 0 0 var(--radius-lg) var(--radius-lg); background: var(--bg-chrome); transform: scaleY(0.45); transform-origin: top; opacity: 0; pointer-events: none; transition: transform 150ms ease, opacity 150ms ease; }
  /* The pointer near the group's top: a hint of the pill, where it will grow. */
  .grab.near .grab-pill::before { opacity: 0.75; }
  .grab-pill :global(svg) { position: relative; opacity: 0; transition: opacity 150ms ease; }
  .grab:hover .grab-pill::before, .grab-pill.open::before, .grab-pill:focus-visible::before { transform: scaleY(1); opacity: 1; }
  .grab:hover .grab-pill :global(svg), .grab-pill.open :global(svg), .grab-pill:focus-visible :global(svg) { opacity: 1; }
  .grab:hover .grab-pill, .grab-pill.open { color: var(--fg); }
  @media (prefers-reduced-motion: reduce) { .grab-pill::before, .grab-pill :global(svg) { transition: none; } }
</style>
