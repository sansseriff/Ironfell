<script lang="ts">
  // A link dropped from a main's port onto one of its parts
  // (component-authoring.md §3): which property the port drives, and how
  // the port's range maps onto it. One binding in the definition:
  // `scale(#port.port.signal, a, b, c, d)`, written through the inspector's
  // bind, which the store sends into the definition.
  import { Popover } from "bits-ui";
  import { controllerManager } from "../controller-manager.svelte";
  import Button from "./ui/Button.svelte";
  import Select from "./ui/Select.svelte";
  import TextInput from "./ui/TextInput.svelte";

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
      const part = await doc.command({ op: 'inspect', node: now.node });
      const port = await doc.command({ op: 'inspect', node: now.spec });
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

  // The popover sits at the drop point: a zero-size anchor there.
  const anchor = $derived(drop ? { getBoundingClientRect: () => new DOMRect(drop.x, drop.y, 0, 0) } : null);

  async function link() {
    if (!drop || !leaf) return;
    const expr = `scale(${drop.spec}.port.signal, ${a}, ${b}, ${c}, ${d})`;
    const r = await doc.command({ op: 'inspect_bind', node: drop.node, leaf: leaf, expr: expr });
    if (!r.ok) {
      error = r.error;
      return;
    }
    doc.linkDrop = null;
  }
</script>

<Popover.Root open={!!drop} onOpenChange={(open) => { if (!open) doc.linkDrop = null; }}>
  <Popover.Portal>
    <Popover.Content class="ui-popover link-popover" customAnchor={anchor} side="bottom" align="start" sideOffset={14} alignOffset={14} aria-label="Link a port">
      <header class="head"><span>Link <strong>{portName}</strong></span></header>
      <div class="ui-popover-body">
        <div class="group"><span class="ui-subheading">Drives</span><Select label="Drives" value={leaf} items={props.map((f) => ({ value: f.leaf, label: labels[f.leaf] ?? f.leaf }))} onChange={pick} /></div>
        <div class="map">
          <span>When {portName} goes from</span>
          <TextInput aria-label="from" bind:value={a} /><span>to</span><TextInput aria-label="to" bind:value={b} />
          <span>{labels[leaf] ?? leaf} goes from</span>
          <TextInput aria-label="drives from" bind:value={c} /><span>to</span><TextInput aria-label="drives to" bind:value={d} />
        </div>
        {#if error}<p class="error">{error}</p>{/if}
      </div>
      <div class="ui-dialog-actions">
        <Button variant="outline" onclick={() => (doc.linkDrop = null)}>Cancel</Button>
        <Button variant="primary" class="primary" onclick={link} disabled={!leaf}>Link</Button>
      </div>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  .head strong { color: var(--component); }
  .group { display: flex; flex-direction: column; gap: 4px; }
  .map { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr); gap: 6px; align-items: center; }
  .map > span { color: var(--fg-muted); font-size: var(--text-sm); }
  .map > span:nth-of-type(odd) { grid-column: 1 / -1; }
  .map :global(.ui-input) { width: 100%; }
  .error { margin: 0; color: var(--danger); overflow-wrap: anywhere; }
</style>
