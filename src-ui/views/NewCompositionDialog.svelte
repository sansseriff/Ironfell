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
    <Dialog.Overlay class="new-comp-overlay" />
    <Dialog.Content class="new-comp-dialog">
      <Dialog.Title class="new-comp-title">New composition</Dialog.Title>
      <form onsubmit={create}>
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
        <div class="actions">
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
  :global(.new-comp-overlay) {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: var(--overlay);
  }
  :global(.new-comp-dialog) {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 1001;
    width: min(360px, calc(100vw - 32px));
    padding: 18px 20px 16px;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: var(--menu-shadow);
    font-size: 13px;
  }
  :global(.new-comp-title) {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
  }
  .field {
    display: grid;
    grid-template-columns: 56px 1fr;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    color: var(--fg-muted);
  }
  .field :global(.ui-input),
  .field :global(.ui-select-trigger) {
    width: 100%;
  }
  .message {
    margin: 2px 0 0 64px;
    color: var(--danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
</style>
