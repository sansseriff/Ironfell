<script lang="ts">
  // The states bar (plans/specs/machines.md §6): on the canvas, over a
  // component whose states are being edited (or whose main is selected), a
  // chip per state. The chip in the record colour is the state on show:
  // edits to the parts go into it. A click on a chip edits that state, a
  // double-click renames it, "+" adds one. While a transition is previewed
  // its two ends are marked instead.
  import { DropdownMenu } from "bits-ui";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ComponentIcon from "@lucide/svelte/icons/component";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import { machines } from "../../lib/machines.svelte";

  const view = $derived(machines.view);
  const m = $derived(view?.machines[0] ?? null);
  const pin = $derived(machines.pinned && view && machines.pinned.def === view.def ? machines.pinned : null);
  // Shown while editing, and as a doorway when a main with states is selected.
  const shown = $derived(!!view && !!m && (machines.editing?.def === view.def || !!view.main));

  let error = $state("");
  async function run(p: Promise<{ ok: boolean; error?: string }>) {
    error = "";
    const r = await p;
    if (!r.ok) error = r.error ?? "That did not work";
  }

  let renaming = $state<string | null>(null);
  function rename(id: string, from: string, to: string) {
    // Enter renames and removes the field, whose blur would rename again.
    if (renaming !== id) return;
    renaming = null;
    const v = to.trim();
    if (v && v !== from) void run(machines.author("rename_state", { state: id, name: v }));
  }
  const focusInput = (el: HTMLInputElement) => {
    el.focus();
    el.select();
  };

  // An edit that changed every state says so for a moment.
  let shared = $state(false);
  let sharedTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const left = machines.sharedAt + 3500 - Date.now();
    if (left <= 0) return;
    shared = true;
    clearTimeout(sharedTimer);
    sharedTimer = setTimeout(() => (shared = false), left);
  });
</script>

{#if shown && view && m}
  <div class="states-bar canvas-ui" role="toolbar" aria-label="States of {view.name ?? 'the component'}" data-states-bar>
    <span class="comp" title="Every instance of {view.name ?? 'this component'} has these states"><ComponentIcon size={14} strokeWidth={1.75} /><span>{view.name ?? "component"}</span></span>
    <span class="rule"></span>
    {#each m.states as st (st.id)}
      {@const s = st.name ?? ""}
      {@const on = !!pin && !pin.scrub && pin.state === s}
      {@const end = !!pin?.scrub && (pin.scrub.from === s || pin.scrub.to === s)}
      {#if renaming === st.id}
        <input class="ui-input rename" aria-label="Rename {s}" value={s} use:focusInput onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") renaming = null; else if (e.key === "Enter") rename(st.id, s, e.currentTarget.value); }} onblur={(e) => rename(st.id, s, e.currentTarget.value)} />
      {:else}
        <div class="chip" class:on class:end data-state={s}>
          <button class="name" aria-pressed={on} onclick={() => run(machines.editState(s))} ondblclick={() => (renaming = st.id)} title={on ? `Editing ${s}` : `Edit ${s}`}>
            {#if on}<span class="dot"></span>{/if}<span class="nm">{s}</span>{#if st.own.length}<span class="count" title="{st.own.length} propert{st.own.length === 1 ? 'y' : 'ies'} set in {s}">{st.own.length}</span>{/if}
          </button>
          {#if on}
            <DropdownMenu.Root>
              <DropdownMenu.Trigger class="more" aria-label="{s} options" title="{s} options"><ChevronDown size={12} strokeWidth={1.75} /></DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content class="ui-menu" side="bottom" sideOffset={6} align="start">
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => (renaming = st.id)}>Rename</DropdownMenu.Item>
                  <DropdownMenu.Item class="ui-menu-item" disabled={m.default === s} onSelect={() => run(machines.author("set_default_state", { state: st.id }))}>{m.default === s ? "Initial state" : "Make initial"}</DropdownMenu.Item>
                  {#if m.states.length > 1}
                    <DropdownMenu.Separator class="ui-menu-separator" />
                    <DropdownMenu.Group>
                      <DropdownMenu.GroupHeading class="ui-menu-heading">Copy the look of</DropdownMenu.GroupHeading>
                      {#each m.states.filter((x) => x.id !== st.id) as o (o.id)}
                        <DropdownMenu.Item class="ui-menu-item" onSelect={() => run(machines.author("copy_pose", { state: st.id, from: o.id }))}>{o.name}</DropdownMenu.Item>
                      {/each}
                    </DropdownMenu.Group>
                  {/if}
                  <DropdownMenu.Separator class="ui-menu-separator" />
                  <DropdownMenu.Item class="ui-menu-item" disabled={m.states.length < 2} onSelect={() => run(machines.author("delete_state", { state: st.id }))}>Delete</DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          {/if}
        </div>
      {/if}
    {/each}
    <IconButton icon={Plus} label="Add a state" onclick={() => run(machines.addState())} />
    {#if error}<span class="ui-badge is-warning" role="alert" title={error}>{error}</span>
    {:else if shared}<span class="ui-badge is-warning" role="status" title="A state holds positions, sizes, colours, opacity and text. This edit changed the drawing itself.">Changed in every state</span>{/if}
    {#if machines.editing}
      <span class="rule"></span>
      <IconButton icon={X} label="Back to the document" onclick={() => machines.close()} />
    {/if}
  </div>
{/if}

<style>
  .states-bar { position: absolute; top: 10px; left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 4px; max-width: calc(100% - 96px); padding: 4px 6px; border: 1px solid var(--border-strong); border-radius: var(--radius-lg, 10px); background: var(--bg-raised, var(--bg)); box-shadow: var(--menu-shadow); color: var(--fg); font-size: var(--text-md); pointer-events: auto; z-index: 4; white-space: nowrap; }
  .comp { display: inline-flex; align-items: center; gap: 5px; padding: 0 4px; color: var(--component, var(--fg-muted)); font-weight: 600; min-width: 0; }
  .comp span { overflow: hidden; text-overflow: ellipsis; max-width: 140px; }
  .rule { width: 1px; height: 18px; margin: 0 2px; background: var(--border); flex: none; }
  .chip { display: inline-flex; align-items: center; height: var(--control-h); border: 1px solid var(--border); border-radius: var(--radius-full); background: var(--bg); overflow: hidden; }
  .chip:hover { border-color: var(--border-strong); }
  .chip.on { border-color: var(--record); background: color-mix(in srgb, var(--record) 20%, var(--bg)); }
  .chip.end { border-color: var(--accent); background: var(--accent-soft); }
  .name { display: inline-flex; align-items: center; gap: 6px; height: 100%; padding: 0 10px; border: 0; background: transparent; color: var(--fg); font: inherit; cursor: pointer; }
  .chip.on .name { padding-right: 4px; font-weight: 600; }
  .nm { max-width: 140px; overflow: hidden; text-overflow: ellipsis; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--record); flex: none; }
  .count { min-width: 16px; padding: 0 4px; border-radius: var(--radius-full); background: color-mix(in srgb, currentColor 12%, transparent); font-size: var(--text-xs); font-weight: 500; font-variant-numeric: tabular-nums; text-align: center; box-sizing: border-box; }
  .chip :global(.more) { display: inline-grid; place-items: center; width: 20px; height: 100%; padding: 0 4px 0 0; border: 0; background: transparent; color: var(--fg-muted); cursor: pointer; }
  .chip :global(.more:hover) { color: var(--fg); }
  .rename { width: 120px; height: var(--control-h); min-height: 0; }
</style>
