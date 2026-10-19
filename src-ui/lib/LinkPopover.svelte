<script lang="ts">
  // A link dropped from a main's port onto one of its parts
  // (component-authoring.md §3): which property the port drives, and how
  // the port's range maps onto it. One binding in the definition:
  // `scale(#port.port.signal, a, b, c, d)`, written through the inspector's
  // bind, which the store sends into the definition.
  import { controllerManager } from "../controller-manager.svelte";

  const doc = controllerManager.docClient;
  const drop = $derived(doc.linkDrop);

  type Field = { leaf: string; field: string; text: string | null; kind: string | null; edit: string };
  let props: Field[] = $state([]);
  let portName = $state("");
  let leaf = $state("");
  let a = $state("0");
  let b = $state("1");
  let c = $state("0");
  let d = $state("1");
  let error = $state("");

  const labels: Record<string, string> = { "transform2d.rot": "Rotation (rad)", "transform2d.x": "X", "transform2d.y": "Y", "transform2d.sx": "Scale X", "transform2d.sy": "Scale Y", "appearance.opacity": "Opacity", "size.w": "Width", "size.h": "Height", "radius.rx": "Radius X", "radius.ry": "Radius Y", "fill.opacity": "Fill opacity", "stroke.width": "Stroke width", "arc.sweep": "Arc sweep" };

  function guess(field: string, current: string) {
    const n = parseFloat(current) || 0;
    if (field === "transform2d.rot") return { c: String(n), d: String(+(n + Math.PI / 2).toFixed(5)) };
    if (field.endsWith("opacity")) return { c: "0.15", d: "1" };
    return { c: String(n), d: String(n === 0 ? 100 : n * 2) };
  }

  $effect(() => {
    const now = drop;
    if (!now) return;
    error = "";
    (async () => {
      const part = await doc.call("inspect", [now.node]);
      const port = await doc.call("inspect", [now.spec]);
      portName = port?.name ?? now.spec;
      // The port's range: from zero to its default (or one), in its unit.
      const signal: string = port?.components?.find((x: any) => x.kind === "port")?.fields?.find((f: Field) => f.field === "signal")?.text ?? "0";
      const m = signal.match(/^(-?[\d.e+-]+)\s*(.*)$/);
      const unit = m?.[2] ?? "";
      const value = m ? parseFloat(m[1]) : 0;
      a = `0${unit ? " " + unit : ""}`;
      b = `${value > 0 ? value * 2 : 1}${unit ? " " + unit : ""}`;
      props = (part?.components ?? [])
        .flatMap((x: any) => x.fields)
        .filter((f: Field) => f.kind === "number" && f.edit !== "locked" && f.leaf in labels);
      leaf = props.find((f) => f.leaf === "transform2d.rot")?.leaf ?? props[0]?.leaf ?? "";
      const g = guess(leaf, props.find((f) => f.leaf === leaf)?.text ?? "0");
      c = g.c;
      d = g.d;
    })().catch((e) => (error = String(e)));
  });

  function pick(next: string) {
    leaf = next;
    const g = guess(next, props.find((f) => f.leaf === next)?.text ?? "0");
    c = g.c;
    d = g.d;
  }

  async function link() {
    if (!drop || !leaf) return;
    const expr = `scale(${drop.spec}.port.signal, ${a}, ${b}, ${c}, ${d})`;
    const r = await doc.call("inspect_bind", [drop.node, leaf, expr]);
    if (!r.ok) {
      error = r.error;
      return;
    }
    doc.linkDrop = null;
  }
</script>

{#if drop}
  <div class="link-popover" role="dialog" aria-label="Link a port" style="left: {Math.min(drop.x + 14, innerWidth - 300)}px; top: {Math.min(drop.y + 14, innerHeight - 260)}px">
    <div class="head">Link <strong>{portName}</strong></div>
    <label>Drives<select value={leaf} onchange={(e) => pick(e.currentTarget.value)}>{#each props as f (f.leaf)}<option value={f.leaf}>{labels[f.leaf] ?? f.leaf}</option>{/each}</select></label>
    <div class="map">
      <span>When {portName} goes from</span>
      <input aria-label="from" bind:value={a} /><span>to</span><input aria-label="to" bind:value={b} />
      <span>{labels[leaf] ?? leaf} goes from</span>
      <input aria-label="drives from" bind:value={c} /><span>to</span><input aria-label="drives to" bind:value={d} />
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button onclick={() => (doc.linkDrop = null)}>Cancel</button>
      <button class="primary" onclick={link} disabled={!leaf}>Link</button>
    </div>
  </div>
{/if}

<style>
  .link-popover {
    position: fixed;
    z-index: 1001;
    width: 280px;
    padding: 12px 14px;
    border: 1px solid var(--border-strong);
    border-radius: 9px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .head strong { color: #7d4dff; }
  label { display: flex; flex-direction: column; gap: 4px; color: var(--fg-muted); font-size: 11px; }
  select, input { min-height: 28px; padding: 0 8px; border: 1px solid var(--border-strong); border-radius: 5px; background: var(--bg); color: var(--fg); font-size: 12px; box-sizing: border-box; }
  .map { display: grid; grid-template-columns: 1fr auto 1fr; gap: 6px; align-items: center; }
  .map span:nth-of-type(odd) { grid-column: 1 / -1; color: var(--fg-muted); font-size: 11px; }
  .map input { width: 100%; }
  .actions { display: flex; justify-content: flex-end; gap: 6px; }
  .primary { background: #7d4dff; color: white; border-color: #7d4dff; }
  .error { color: var(--danger); margin: 0; overflow-wrap: anywhere; }
</style>
