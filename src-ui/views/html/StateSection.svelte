<script lang="ts">
  // The Inspector while a component's states are edited and nothing is
  // selected (machines.md §6): the state on show (its name, whether
  // instances start in it, how much it sets), or the transition previewed
  // (its name, length and easing).
  import Section from "../../lib/ui/Section.svelte";
  import EasingField from "../../lib/ui/EasingField.svelte";
  import Checkbox from "../../lib/ui/Checkbox.svelte";
  import { machines } from "../../lib/machines.svelte";

  const view = $derived(machines.view);
  const pin = $derived(machines.pinned);
  const m = $derived(view?.machines.find((x) => x.id === pin?.machine) ?? null);
  const st = $derived(pin && !pin.scrub ? m?.states.find((s) => s.name === pin.state) : undefined);
  const pair = $derived(pin?.scrub && m ? m.pairs.find((q) => q.from === pin.scrub!.from && q.to === pin.scrub!.to) : undefined);
  const t = $derived(pair ? m?.transitions.find((x) => x.id === pair.transition) : undefined);
  const own = $derived(!!t && !!pair && t.from === pair.from && t.to === pair.to);

  let error = $state("");
  async function run(p: Promise<{ ok: boolean; error?: string }>) {
    error = "";
    const r = await p;
    if (!r.ok) error = r.error ?? "That did not work";
  }
  const text = (e: Event) => (e.currentTarget as HTMLInputElement).value.trim();
  function editPair(edit: { ms?: number; ease?: string; name?: string }, label: string) {
    if (view && m && pair) void run(machines.editPair(view.def, m.id, pair.from, pair.to, edit, label));
  }
</script>

{#if view && m && st}
  <Section title={st.name ?? "State"} class="property-section" data-section="state" meta="state of {view.name ?? 'component'}">
    <label class="ui-field name"><span>Name</span><input aria-label="State name" value={st.name ?? ""} onchange={(e: Event) => { const v = text(e); if (v && v !== st.name) void run(machines.author("rename_state", { state: st.id, name: v })); }} /></label>
    <div class="line"><Checkbox label="Initial state" checked={m.default === st.name} disabled={m.default === st.name} onChange={() => run(machines.author("set_default_state", { state: st.id }))} /><span class="ui-badge" title="Properties this state sets">{st.own.length} set</span></div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </Section>
{:else if view && m && pair}
  <Section title="{pair.from} → {pair.to}" class="property-section" data-section="transition" meta={t ? (own ? "own timing" : "default timing") : "snaps"}>
    {#if t}
      <label class="ui-field name"><span>Name</span><input aria-label="Transition name" placeholder="None" value={own && t.name && t.name !== `${t.from} to ${t.to}` && t.name !== "change" ? t.name : ""} onchange={(e: Event) => { const v = text(e); if (v) editPair({ name: v }, "name transition"); }} /></label>
      <div class="property-grid">
        <label class="ui-field"><span>Length</span><input inputmode="decimal" aria-label="Transition length, seconds" value={(pair.ms ?? t.ms ?? 0) / 1000} onchange={(e) => { const v = parseFloat(e.currentTarget.value); if (Number.isFinite(v) && v >= 0) editPair({ ms: v * 1000 }, "transition length"); }} /><span>s</span></label>
        <EasingField label="Transition easing" value={t.ease} onChange={(v) => editPair({ ease: v }, "transition easing")} />
      </div>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </Section>
{/if}

<style>
  .ui-field { gap: 6px; padding: 0 8px; color: var(--fg-muted); font-size: var(--text-sm); }
  .ui-field input { flex: 1; width: 100%; min-width: 0; border: 0; background: transparent; color: var(--fg); font: inherit; font-size: var(--text-md); outline: none; text-align: right; }
  .name { width: 100%; box-sizing: border-box; }
  .line { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: var(--control-h); }
  .error { margin: 0; color: var(--danger); font-size: var(--text-sm); }
</style>
