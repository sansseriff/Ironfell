<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import Settings from "@lucide/svelte/icons/settings";
  import Check from "@lucide/svelte/icons/check";
  import { showSettings } from "./workspace";
  import { settings, type Appearance } from "../runtime/settings.svelte";
  import { setAppearance } from "../runtime/theme.svelte";

  const appearances: { value: Appearance; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
</script>

<div class="global-settings canvas-ui">
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class="ui-icon-button is-nav" aria-label="Settings" title="Settings">
      <Settings size={16} strokeWidth={1.75} />
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content class="ui-menu" side="right" align="end" sideOffset={8}>
        <DropdownMenu.Item class="ui-menu-item" onSelect={showSettings}>App settings…</DropdownMenu.Item>
        <DropdownMenu.Separator class="ui-menu-separator" />
        <DropdownMenu.Group>
          <DropdownMenu.GroupHeading class="ui-menu-heading">Appearance</DropdownMenu.GroupHeading>
          {#each appearances as item (item.value)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => setAppearance(item.value)}>
              <span class="ui-menu-check">{#if settings.appearance === item.value}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>{item.label}</span>
            </DropdownMenu.Item>
          {/each}
        </DropdownMenu.Group>
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
</div>

<style>
  /* A single workspace action at the foot of the left view selector. It
     remains reachable even when that group's tabs are hidden or closed. */
  .global-settings { position: absolute; left: 0; bottom: 0; width: 40px; height: 44px; display: flex; align-items: center; justify-content: center; box-sizing: border-box; border-right: 1px solid var(--border); background: var(--bg-rail); pointer-events: auto; z-index: 6; }
</style>
