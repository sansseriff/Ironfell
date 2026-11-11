<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import Button from "./ui/Button.svelte";
  import Select from "./ui/Select.svelte";
  import TextInput from "./ui/TextInput.svelte";

  const doc = controllerManager.docClient;
  type Point = { id: string; index: number; attachment: null | { control: string; dx: number; dy: number } };
  type Info = { ok: boolean; error?: string; main: string; name: string; controls: { id: string; name: string; port: boolean }[]; paths: { id: string; name: string; packed: boolean; points: Point[] }[]; picked_point: string | null };
  let owner = $state("");
  let control = $state("");
  let info = $state<Info | null>(null);
  let picked = $state<string[]>([]);
  let error = $state("");
  let busy = $state(false);
  let serial = 0;
  let session = 0;
  let epoch = 0;
  let offsetX = $state("0");
  let offsetY = $state("0");
  let editing = $state("");
  const points = $derived(info?.paths.flatMap(p => p.points) ?? []);
  const attached = $derived(points.filter(p => p.attachment?.control === control));
  const candidate = $derived(points.find(p => p.id === info?.picked_point && !p.attachment));

  async function call(verb: string, args: Record<string, unknown>) {
    return await doc.command({ op: "author", verb, args: JSON.stringify(args) });
  }
  async function refresh() {
    if (!owner || busy) return;
    const request = ++serial, target = owner, generation = doc.generation, documentEpoch = doc.documentEpoch;
    try {
      const r: Info = await call("attachment_info", { target });
      if (request !== serial || owner !== target || generation !== doc.generation || documentEpoch !== doc.documentEpoch) return;
      info = r.ok ? r : null;
      if (!r.ok) { error = r.error ?? "The workflow target is no longer available. Cancel and select a main."; return; }
      if (control && !r.controls.some(c => c.id === control)) { control = ""; error = "The control was removed. Choose another control."; }
      picked = picked.filter(id => r.paths.some(p => p.points.some(q => q.id === id && !q.attachment)));
    } catch (e) { if (request === serial) error = String(e); }
  }
  // The inspector's Attach points button starts it (bind:this).
  export function begin() { if (!busy) void start(); }
  export function isOpen() { return !!owner; }
  async function start() {
    error = "";
    const target = doc.selection[0];
    if (!target) return;
    busy = true;
    const generation = doc.generation;
    const documentEpoch = doc.documentEpoch;
    try {
      const r: Info = await call("attachment_info", { target });
      if (generation !== doc.generation || documentEpoch !== doc.documentEpoch) return;
      if (!r.ok) { error = r.error ?? "Select a main component"; return; }
      session = generation; epoch = documentEpoch; owner = r.main; info = r; picked = [];
      control = r.controls.some(c => c.id === target) ? target : (r.controls[0]?.id ?? "");
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
  function close() { serial++; owner = ""; control = ""; info = null; picked = []; error = ""; editing = ""; }
  $effect(() => { if (owner && (doc.generation !== session || doc.documentEpoch !== epoch)) close(); });
  // A refusal ("Select a main component") lasts until the selection changes.
  $effect(() => { void doc.selection; untrack(() => { if (!owner) error = ""; }); });
  const timer = setInterval(refresh, 400);
  onDestroy(() => { serial++; clearInterval(timer); });
  function toggle(id: string) { picked = picked.includes(id) ? picked.filter(p => p !== id) : [...picked, id]; }
  async function perform(verb: string, args: Record<string, unknown>) {
    busy = true; error = ""; serial++;
    try {
      const r = await call(verb, args);
      if (!r.ok) error = r.error ?? "The operation could not be applied";
      else { if (verb === "attach_points") picked = []; editing = ""; }
    } catch (e) { error = String(e); }
    finally { busy = false; await refresh(); }
  }
  async function offsets(p: Point) {
    const x = Number(offsetX), y = Number(offsetY);
    if (!Number.isFinite(x) || !Number.isFinite(y) || !offsetX.trim() || !offsetY.trim()) { error = "Enter two finite offsets"; return; }
    busy = true; error = "";
    try {
      const r = await doc.command({ op: "inspect_patch", node: p.id, patch: JSON.stringify({ values: { "attachment.dx": String(x), "attachment.dy": String(y) }, label: "edit attachment offset" }) });
      if (!r.ok) error = r.error; else editing = "";
    } catch (e) { error = String(e); }
    finally { busy = false; await refresh(); }
  }
</script>

<!-- Working, the task sits under the inspector's title (its button is the
     title's Attach points action); it outlives the selection. -->
{#if owner || error}
<div class="authoring" data-attachment-task>
  {#if owner}
    <header class="ui-section-header"><span>Attach points</span><Button small onclick={close} disabled={busy}>Done</Button></header>
    <p>Editing main {info?.name || owner}. This target stays pinned while you pick geometry.</p>
    <label>Endpoint control
      <Select value={control} items={(info?.controls ?? []).map(c => ({ value: c.id, label: c.name || c.id }))} onChange={v => { control = v; editing = ""; }} label="Endpoint control" disabled={busy} />
    </label>
    <p>Choose an empty, then collect points from both paths. Apply keeps their current positions. Offsets follow the control’s rotation and scale; tangents stay in the path’s frame.</p>
    {#if !info?.controls.length}<p>Add an Empty inside this main using the shape toolbar, then return here.</p>{/if}
    {#if candidate}<Button small onclick={() => toggle(candidate!.id)} disabled={busy}> {picked.includes(candidate.id) ? "Remove" : "Collect"} canvas point {candidate.index}</Button>{/if}
    <div class="paths">
      {#each info?.paths ?? [] as path (path.id)}
        <div class="path">
          <strong>{path.name || "Path"}</strong><span class="id">{path.id}</span>
          {#if path.packed}
            <Button small disabled={busy} onclick={() => perform("expand_path_points", { path: path.id })}>Enable point attachments</Button>
            <p>Creates editable points for this path only. Undo restores its packed geometry.</p>
          {:else}
            <div class="point-list">
              {#each path.points as point (point.id)}
                <Button small variant={picked.includes(point.id) ? "primary" : "outline"} disabled={busy || !!point.attachment} aria-pressed={picked.includes(point.id)} title={point.attachment ? "Already attached; detach below to reassign" : "Add or remove this point from the pending attachment"} onclick={() => toggle(point.id)}>Point {point.index}</Button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="actions">
      <Button variant="primary" disabled={busy || !control || !picked.length} onclick={() => perform("attach_points", { control, points: picked })}>Attach {picked.length} point{picked.length === 1 ? "" : "s"}</Button>
      <Button disabled={busy || !picked.length} onclick={() => { picked = []; }}>Clear picks</Button>
    </div>
    <p>Picking changes nothing until Attach. Each Attach is one undo step.</p>
    {#if attached.length}
      <strong>Controlled points ({attached.length})</strong>
      {#each attached as point (point.id)}
        <div class="attached">
          <span>{info?.paths.find(p => p.points.some(q => q.id === point.id))?.name || "Path"} · point {point.index}</span>
          {#if editing === point.id}
            <label>X offset<TextInput aria-label="Attachment X offset" bind:value={offsetX} /></label>
            <label>Y offset<TextInput aria-label="Attachment Y offset" bind:value={offsetY} /></label>
            <Button small disabled={busy} onclick={() => offsets(point)}>Save offset</Button>
            <Button small disabled={busy} onclick={() => editing = ""}>Cancel offset</Button>
          {:else}
            <span class="id">Offset {point.attachment!.dx.toFixed(2)}, {point.attachment!.dy.toFixed(2)}</span>
            <Button small disabled={busy} onclick={() => { editing = point.id; offsetX = String(point.attachment!.dx); offsetY = String(point.attachment!.dy); }}>Edit offset</Button>
            <Button small disabled={busy} onclick={() => perform("detach_points", { points: [point.id] })}>Detach</Button>
          {/if}
        </div>
      {/each}
      <Button small disabled={busy} onclick={() => perform("detach_points", { points: attached.map(p => p.id) })}>Detach all from control</Button>
      <p>Detach preserves the current geometry. Detach points before deleting their control.</p>
    {/if}
    {#if control && !info?.controls.find(c => c.id === control)?.port}
      <div class="actions">
        <Button small disabled={busy} onclick={() => perform("expose_control", { control, dir: "in" })}>Expose as input port</Button>
        <Button small disabled={busy} onclick={() => perform("expose_control", { control, dir: "out" })}>Expose as output port</Button>
      </div>
      <p>Creates a loose endpoint. Select its empty to set kind, connector, facing and signal in the inspector.</p>
    {/if}
    <Button small disabled={busy} onclick={close}>Cancel pending picks</Button>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>
{/if}

<style>
  .authoring { display: flex; flex-direction: column; gap: 8px; padding: 12px var(--panel-pad); border-bottom: 1px solid var(--border); font-size: var(--text-md); }
  .actions, .point-list { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; }
  p { margin: 0; color: var(--fg-muted); line-height: 1.5; }
  label { display: flex; flex-direction: column; gap: 4px; }
  .paths { max-height: 240px; overflow: auto; }
  .path, .attached { display: flex; flex-direction: column; gap: 6px; padding: 8px; margin-bottom: 4px; border: 1px solid var(--border); border-radius: var(--radius); }
  .id { color: var(--fg-muted); font-family: var(--font-mono); font-size: var(--text-xs); overflow-wrap: anywhere; }
  .error { color: var(--danger); overflow-wrap: anywhere; }
</style>
