<script lang="ts">
  import { LinkPreview } from "bits-ui";
  import Settings from "@lucide/svelte/icons/settings";
  import Check from "@lucide/svelte/icons/check";
  import SettingsDialog from "./SettingsDialog.svelte";
  import { settings, type Appearance } from "../runtime/settings.svelte";
  import { setAppearance } from "../runtime/theme.svelte";

  let settingsOpen = $state(false);
  let hoverOpen = $state(false);

  const appearances: { value: Appearance; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
</script>

<!-- Click opens the settings dialog; hovering the gear shows the appearance
     choices, which need no label of their own. -->
<div class="global-settings canvas-ui">
  <LinkPreview.Root bind:open={hoverOpen} openDelay={250} closeDelay={150}>
    <LinkPreview.Trigger>
      {#snippet child({ props })}
        <button
          {...props}
          type="button"
          class="ui-icon-button is-nav"
          aria-label="Settings"
          title="Settings"
          onclick={() => { hoverOpen = false; settingsOpen = true; }}
        >
          <Settings size={16} strokeWidth={1.75} />
        </button>
      {/snippet}
    </LinkPreview.Trigger>
    <LinkPreview.Portal>
      <LinkPreview.Content class="ui-menu appearance-menu" side="right" align="end" sideOffset={8}>
        {#each appearances as item (item.value)}
          <button
            type="button"
            class="ui-menu-item"
            aria-pressed={settings.appearance === item.value}
            onclick={() => setAppearance(item.value)}
          >
            <span class="ui-menu-check">{#if settings.appearance === item.value}<Check size={14} strokeWidth={1.75} />{/if}</span>
            <span>{item.label}</span>
          </button>
        {/each}
      </LinkPreview.Content>
    </LinkPreview.Portal>
  </LinkPreview.Root>
</div>
<SettingsDialog bind:open={settingsOpen} />

<style>
  /* A single workspace action at the foot of the left view selector. It
     remains reachable even when that group's tabs are hidden or closed. */
  .global-settings { position: absolute; left: 0; bottom: 0; width: 40px; height: 44px; display: flex; align-items: center; justify-content: center; box-sizing: border-box; border-right: 1px solid var(--border); background: var(--bg-rail); pointer-events: auto; z-index: 6; }
  .global-settings :global(.is-nav) { width: 32px; height: 32px; border-radius: var(--radius-lg); }
  .global-settings :global(.is-nav:hover), .global-settings :global(.is-nav[aria-expanded="true"]) { background: var(--hover); color: var(--fg); }
  /* Plain buttons, not menu items, so they take the menu's highlight on hover
     and focus themselves. */
  :global(.appearance-menu) { min-width: 140px; }
  :global(.appearance-menu .ui-menu-item) { width: 100%; border: 0; background: none; color: inherit; font: inherit; text-align: left; }
  :global(.appearance-menu .ui-menu-item:hover),
  :global(.appearance-menu .ui-menu-item:focus-visible) { background: var(--accent); color: var(--accent-fg); }
</style>
