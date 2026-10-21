<script lang="ts">
  // Composition settings (plans/specs/free-running.md §3): a button in the
  // view's top right corner. Run: Auto (free running while the composition
  // or anything it shows is live, as the engine reports), Always, or On
  // demand (never; what it shows does not bubble out, and producers seen
  // only through it pause).
  import { DropdownMenu } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { onDestroy } from "svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";

  let { spec }: { spec: ViewSpec } = $props();

  const doc = controllerManager.docClient;

  type Run = "auto" | "always" | "on_demand";
  type Info = { ok: boolean; run: Run; live: boolean; running: string[] };
  let info = $state<Info | null>(null);
  let open = $state(false);

  const MODES: { id: Run; label: string; hint: string }[] = [
    { id: "auto", label: "Auto", hint: "Runs while something in it is live" },
    { id: "always", label: "Always", hint: "Renders every frame" },
    { id: "on_demand", label: "On demand", hint: "Renders on input; live parts pause" },
  ];

  async function refresh() {
    try {
      const r: Info = await doc.call("composition_info", [spec.source]);
      info = r?.ok ? r : null;
    } catch {
      info = null;
    }
  }

  // Liveness changes without a document version (a producer starts or is
  // frozen), so re-read while the menu is open as well as on each version.
  $effect(() => {
    void doc.version;
    void spec.source;
    refresh();
  });
  const poll = setInterval(() => {
    if (open) refresh();
  }, 500);
  onDestroy(() => clearInterval(poll));

  async function set(run: Run) {
    await doc.call("set_run_mode", [spec.source, run]);
    await refresh();
  }

  const status = $derived(
    !info ? "" : info.live ? (info.running.length ? `Running: ${info.running.join(", ")}` : "Running") : "Frozen until input",
  );
</script>

{#if info}
  <div class="settings canvas-ui">
    <DropdownMenu.Root bind:open onOpenChange={(o) => o && refresh()}>
      <DropdownMenu.Trigger class="trigger" title="Composition settings" aria-label="Composition settings">
        <Settings2 size={16} strokeWidth={1.75} />
        {#if info.live}<span class="live" aria-hidden="true"></span>{/if}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class="ui-menu" side="bottom" sideOffset={6} align="end">
          <div class="ui-menu-heading">Run</div>
          {#each MODES as m (m.id)}
            <DropdownMenu.Item class="ui-menu-item" onSelect={() => set(m.id)} data-run={m.id}>
              <span class="check">{#if info.run === m.id}<Check size={14} strokeWidth={2} />{/if}</span>
              <span>{m.label}</span>
              <span class="ui-hint">{m.hint}</span>
            </DropdownMenu.Item>
          {/each}
          <div class="ui-menu-separator"></div>
          <div class="ui-menu-heading status" data-status>{status}</div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  </div>
{/if}

<style>
  .settings {
    position: absolute;
    top: 8px;
    right: 8px;
  }
  .settings :global(.trigger) {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg);
    color: var(--fg-muted);
    box-shadow: var(--menu-shadow);
    cursor: pointer;
  }
  .settings :global(.trigger:hover) {
    color: var(--fg);
    background: var(--hover);
  }
  .live {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .check {
    display: flex;
    width: 14px;
  }
  .status {
    text-transform: none;
    letter-spacing: 0;
  }
</style>
