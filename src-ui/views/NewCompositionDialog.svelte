<script lang="ts">
  // New composition (the "+" menu): a name no other composition has, and
  // what kind it is: a 2D drawing, a 3D scene, or a 2D schematic. One
  // undoable "new composition" transaction (engine_api `author`,
  // `new_composition`); `onCreated` opens a view of it.
  import { Dialog } from "bits-ui";
  import Button from "../lib/ui/Button.svelte";
  import Select, { type SelectItem } from "../lib/ui/Select.svelte";
  import TextInput from "../lib/ui/TextInput.svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import type { CompositionEntry } from "../runtime/document_client.svelte";

  let { open = $bindable(false), onCreated }: { open?: boolean; onCreated: (c: CompositionEntry) => void } = $props();

  type Kind = "2d" | "3d" | "schematic";
  const KINDS: SelectItem<Kind>[] = [
    { value: "2d", label: "2D drawing" },
    { value: "3d", label: "3D scene" },
    { value: "schematic", label: "Schematic", hint: "2D, wired parts" },
  ];

  const doc = controllerManager.docClient;
  let name = $state("");
  let kind: Kind = $state("2d");
  let error = $state("");
  let busy = $state(false);
  let input: HTMLInputElement | HTMLTextAreaElement | undefined = $state();

  const trimmed = $derived(name.trim());
  const taken = $derived(!!trimmed && doc.compositions.some((c) => c.name === trimmed));

  /** A free name to start from: composition, composition 2, … */
  function suggestion(): string {
    const names = new Set(doc.compositions.map((c) => c.name));
    if (!names.has("composition")) return "composition";
    for (let i = 2; ; i++) if (!names.has(`composition ${i}`)) return `composition ${i}`;
  }

  $effect(() => {
    if (open) {
      name = suggestion();
      kind = "2d";
      error = "";
      queueMicrotask(() => input?.select());
    }
  });

  async function create(e?: Event) {
    e?.preventDefault();
    if (!trimmed || taken || busy) return;
    busy = true;
    error = "";
    try {
      const args = { name: trimmed, dims: kind === "3d" ? "3d" : "2d", notation: kind === "schematic" ? "schematic" : "free" };
      const r = await doc.command<{ ok: boolean; made?: string; error?: string }>({ op: "author", verb: "new_composition", args: JSON.stringify(args) });
      if (!r.ok || !r.made) {
        error = r.error ?? "The composition could not be made.";
        return;
      }
      await doc.refreshCompositions();
      open = false;
      onCreated({ id: r.made, name: trimmed, dims: args.dims, session: false });
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="ui-dialog-overlay" />
    <Dialog.Content class="ui-dialog">
      <Dialog.Title class="ui-dialog-title">New composition</Dialog.Title>
      <form onsubmit={create}>
        <div class="ui-dialog-body">
          <label class="field">
            <span>Name</span>
            <TextInput bind:value={name} bind:element={input} aria-label="Composition name" spellcheck="false" autocomplete="off" />
          </label>
          <div class="field">
            <span>Kind</span>
            <Select value={kind} items={KINDS} label="Kind" onChange={(v) => (kind = v)} />
          </div>
          {#if taken}
            <p class="message">A composition named “{trimmed}” already exists.</p>
          {:else if error}
            <p class="message">{error}</p>
          {/if}
        </div>
        <div class="ui-dialog-actions">
          <Dialog.Close>
            {#snippet child({ props })}
              <Button variant="outline" {...props}>Cancel</Button>
            {/snippet}
          </Dialog.Close>
          <Button variant="primary" type="submit" disabled={!trimmed || taken || busy}>Create</Button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .field {
    display: grid;
    grid-template-columns: 56px 1fr;
    align-items: center;
    gap: 8px;
    color: var(--fg-muted);
  }
  .message {
    margin: 0 0 0 64px;
    color: var(--danger);
  }
</style>
