<script lang="ts">
  // A narrative (source: its name or id, notation: narrative), the panel of
  // plans/specs/narrative-panel.md: the document (a ProseMirror editor over
  // the narrative's blocks, src-ui/narrative/editor.ts) and the
  // conversation, laid out in one of three modes (§4.1):
  //
  //   Authoring    the document fills the panel; a small composer below
  //   Hybrid       the document above, the conversation below
  //   Exploration  the conversation fills the panel; the document an outline
  //
  // The composer shows where the model will write ("at the cursor, after
  // §…", or "conversation only"); a model turn's changes to the narrative are
  // marked for review, each kept or reverted (§7 item 6).
  import { getContext, onDestroy, onMount } from "svelte";
  import { Button as BitsButton, DropdownMenu, ToggleGroup } from "bits-ui";
  import Pilcrow from "@lucide/svelte/icons/pilcrow";
  import Heading1 from "@lucide/svelte/icons/heading-1";
  import Heading2 from "@lucide/svelte/icons/heading-2";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import TextQuote from "@lucide/svelte/icons/text-quote";
  import Code from "@lucide/svelte/icons/code";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Variable from "@lucide/svelte/icons/variable";
  import SquareCode from "@lucide/svelte/icons/square-code";
  import Download from "@lucide/svelte/icons/download";
  import Upload from "@lucide/svelte/icons/upload";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import type { Component } from "svelte";
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import TextInput from "../../lib/ui/TextInput.svelte";
  import EmptyState from "../../lib/ui/EmptyState.svelte";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import SquarePlay from "@lucide/svelte/icons/square-play";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import ChatPanel from '../../chat/ChatPanel.svelte';
  import { panelHistory } from '../../chat/sessions.svelte';
  import { narrativeMode, type ChatHandle, type ChatTurn, type NarrativeMode as Mode } from '../../chat/conversation';
  import type { ConversationMessage } from '../../model/conversation';
  import type { AskLive } from '../../model/progress';
  import type { AskResult } from '../../model/agent';
  import Select from "../../lib/ui/Select.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";
  import { showComposition } from "../workspace";
  import { NarrativeEditor, type CursorInfo, type MentionQuery } from "../../narrative/editor";
  import { diff, sameContent, type StoredBlock } from "../../narrative/convert";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

  const MODES = [
    { value: "authoring", label: "Authoring: the document, a small composer", icon: PenLine },
    { value: "hybrid", label: "Hybrid: the document above, the conversation below", icon: Rows2 },
    { value: "exploration", label: "Exploration: the conversation, the document an outline", icon: MessagesSquare },
  ] as const;

  const doc = controllerManager.docClient;
  const active = getContext<() => boolean>("iron-view-active") ?? (() => true);
  // svelte-ignore state_referenced_locally
  const modeKey = `iron.narrative.mode.${panel ?? spec.source}`;
  let mode: Mode = $state((() => {
    try {
      return narrativeMode(localStorage.getItem(modeKey));
    } catch {
      return "authoring";
    }
  })());
  $effect(() => {
    try {
      localStorage.setItem(modeKey, mode);
    } catch {
      // storage unavailable
    }
  });

  // svelte-ignore state_referenced_locally
  let source = $state(spec.source);
  let host: HTMLDivElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let editor: NarrativeEditor | null = null;
  let missing = $state(false);
  let status = $state({ pending: false, error: "", version: 0, blocks: 0 });
  let cursor: CursorInfo = $state({ block: null, index: 0, heading: "", selection: "" });
  let narratives: { id: string; name: string; blocks: number }[] = $state([]);
  let outline: { id: string; text: string; level: number; view: boolean }[] = $state([]);
  let follow = $state(true);

  // ---- the mention menu ----------------------------------------------
  let mention: MentionQuery | null = $state(null);
  let targets: { id: string; name: string; type: string | null; composition: string | null }[] = $state([]);
  let pick = $state(0);
  let mentionRequest = 0;
  $effect(() => {
    const q = mention;
    if (!q) {
      targets = [];
      return;
    }
    const n = ++mentionRequest;
    void doc.command<{ ok: boolean; targets: typeof targets }>({ op: "narrative_targets", query: q.query }).then((r) => {
      if (n === mentionRequest) {
        targets = r.ok ? r.targets : [];
        pick = 0;
      }
    });
  });
  function choose(i: number) {
    const t = targets[i];
    if (!t || !mention || !editor) return;
    editor.insertMention(mention, t.id, t.name);
    mention = null;
  }

  // ---- the editor's host ----------------------------------------------
  function makeEditor() {
    if (!host) return;
    editor?.destroy();
    host.replaceChildren();
    const e = new NarrativeEditor(host, {
      read: (s) => doc.command({ op: "narrative_read", source: s }),
      edit: (s, edits, label) => doc.command({ op: "narrative_edit", source: s, edits: JSON.stringify(edits), label, actor: "human" }),
      values: (s) => doc.command({ op: "narrative_values", source: s }),
      undo: () => doc.undo(),
      redo: () => doc.redo(),
      probe: (node, action) => void doc.command({ op: "narrative_probe", node, action }),
      showView: (show, focus) => {
        showComposition(show);
        if (focus) void doc.command({ op: "focus", node: focus });
      },
    }, source);
    e.onStatus = (s) => {
      status = s;
      missing = !!s.error && /no narrative/.test(s.error);
      outline = readOutline(e);
    };
    e.onCursor = (c) => (cursor = c);
    e.onMention = (q) => (mention = q);
    e.mentionKeys = (ev) => {
      if (!mention || !targets.length) return false;
      if (ev.key === "ArrowDown") pick = (pick + 1) % targets.length;
      else if (ev.key === "ArrowUp") pick = (pick + targets.length - 1) % targets.length;
      else if (ev.key === "Enter" || ev.key === "Tab") choose(pick);
      else if (ev.key === "Escape") mention = null;
      else return false;
      return true;
    };
    editor = e;
    registry.set(panel ?? source, api());
    void e.refresh();
  }

  function readOutline(e: NarrativeEditor) {
    const out: { id: string; text: string; level: number; view: boolean }[] = [];
    e.view.state.doc.forEach((n) => {
      if (n.type.name === "heading" || n.type.name === "view") out.push({ id: n.attrs.id, text: n.type.name === "view" ? n.attrs.show : n.textContent, level: n.type.name === "heading" ? n.attrs.level : 3, view: n.type.name === "view" });
    });
    return out;
  }

  onMount(() => {
    makeEditor();
    void listNarratives();
    void loadCompositions().catch(() => {});
  });
  onDestroy(() => {
    void editor?.flush();
    editor?.destroy();
    registry.delete(panel ?? source);
    observer?.disconnect();
  });

  // The store changed (any edit, undo, a model turn): read it again.
  $effect(() => {
    void doc.version;
    void doc.generation;
    if (!active()) return;
    void editor?.refresh();
    if (!narratives.length) void listNarratives();
  });
  // Signals moved without a new version: values only.
  $effect(() => {
    void doc.resolvedVersion;
    if (!active()) return;
    void editor?.refreshValues();
  });

  async function listNarratives() {
    const r = await doc.command<{ ok: boolean; narratives: typeof narratives }>({ op: "narrative_list" }).catch(() => null);
    narratives = r?.ok ? r.narratives : [];
  }

  async function createNarrative() {
    const name = newName.trim() || "notes";
    const r = await doc.command<{ ok: boolean; id?: string; error?: string }>({ op: "narrative_create", name, markdown: `# ${name[0].toUpperCase()}${name.slice(1)}\n\n`, actor: "human" });
    if (!r.ok) {
      status = { ...status, error: r.error ?? "could not create" };
      return;
    }
    source = name;
    missing = false;
    await listNarratives();
    makeEditor();
  }
  let newName = $state("");

  function open(name: string) {
    if (name === source) return;
    void editor?.flush();
    source = name;
    makeEditor();
  }

  // ---- view blocks follow the reading position ------------------------
  let observer: IntersectionObserver | null = null;
  $effect(() => {
    void status.version;
    void follow;
    observer?.disconnect();
    if (!scroller || !editor || !follow || mode === "exploration") return;
    // A view block entering the top quarter of the panel shows its composition.
    // The observer's first report is where the blocks already are, not a
    // scroll: opening the panel (or an edit) never switches the view.
    let first = true;
    observer = new IntersectionObserver(
      (entries) => {
        if (first) {
          first = false;
          return;
        }
        for (const en of entries) {
          const el = en.target as HTMLElement;
          if (en.isIntersecting && el.dataset.show && editor && editor.shown !== el.dataset.show) editor.showView(el.dataset.show, el.dataset.focus ?? "");
        }
      },
      { root: scroller, rootMargin: "0px 0px -75% 0px", threshold: 0 },
    );
    for (const el of editor.viewBlocks()) observer.observe(el);
  });

  // ---- the conversation ----------------------------------------------
  interface NarrativeReview {
    source: string;
    before: StoredBlock[];
    after: StoredBlock[];
    changed: string[];
    reviewed: "" | "kept" | "reverted";
    error?: string;
  }
  // This narrative's chats, saved with the scope (chat/sessions.svelte.ts).
  const history = $derived(panelHistory(`narrative:${source}`));
  const session = $derived(history.current);
  const reviews = $derived(session.annotations as Record<string, NarrativeReview>);
  let chat: ChatHandle | undefined = $state();
  let writeAtCursor = $state(true);
  const cursorLabel = $derived(
    cursor.selection
      ? `Edits the selection${cursor.heading ? ` in “${cursor.heading}”` : ""}`
      : `Writes at the cursor${cursor.heading ? `, in “${cursor.heading}”` : ""}`,
  );
  $effect(() => {
    // Each mode's default target (§4.1): Authoring writes at the cursor,
    // Exploration talks only.
    writeAtCursor = mode !== "exploration";
  });
  const targetLabel = $derived(writeAtCursor ? cursorLabel : "Conversation only");

  /** The request with where the author is: a hint, not a rule (§4.2). */
  function contextFor(text: string): string {
    if (!writeAtCursor) return `${text}\n\n(The author is talking, not writing: change the narrative "${source}" only if asked to.)`;
    const at = cursor.block ? ` after block [${cursor.block}]` : " at its end";
    const sel = cursor.selection ? `\nThe author has selected: "${cursor.selection.slice(0, 400)}". Keep the edit to the selection unless the request names somewhere else.` : "";
    return `${text}\n\n(The author's cursor is in the narrative "${source}"${at}${cursor.heading ? `, under the heading "${cursor.heading}"` : ""}. Insert new text there with <write narrative="${source}" after="${cursor.block ?? "end"}">; edits elsewhere are allowed when the request implies them.)${sel}`;
  }

  async function performTurn(text: string, history: ConversationMessage[], turn: ChatTurn, live: AskLive = {}, xmlForTest?: string): Promise<AskResult> {
    const instruction = contextFor(text);
    const owner = session;
    const currentSource = source;
    const currentEditor = editor;
    const generation = doc.generation;
    await editor?.flush();
    const before = currentEditor?.storedBlocks() ?? [];
    let result: AskResult;
    if (xmlForTest !== undefined) {
      const outcome = await doc.edit(xmlForTest, `model: ${text.slice(0, 60)}`);
      result = { ok: !!outcome.ok, note: '', edit: xmlForTest, outcome, attempts: 1, transport: 'scripted' };
    } else {
      result = await doc.ask(instruction, { history, allowTextReply: true, turnId: turn.id, images: turn.images }, live);
    }
    if (generation !== doc.generation) return result;
    const read = await doc.command<{ ok: boolean; blocks: StoredBlock[] }>({ op: 'narrative_read', source: currentSource });
    const after = read.ok ? read.blocks : [];
    const was = new Map(before.map(block => [block.id, block]));
    const changed = after.filter(block => !was.has(block.id) || !sameContent(was.get(block.id)!, block)).map(block => block.id);
    if (before.some(block => !after.find(next => next.id === block.id))) changed.push('(deleted)');
    owner.annotations[turn.id] = { source: currentSource, before, after, changed, reviewed: '' } satisfies NarrativeReview;
    if (editor === currentEditor && source === currentSource) {
      await editor?.refresh();
      editor?.highlight(new Set(changed));
      if (changed[0] && changed[0] !== '(deleted)') editor?.reveal(changed[0]);
    }
    return result;
  }

  function keep(index: number) {
    const review = reviews[session.turns[index]?.id];
    if (!review) return;
    review.reviewed = 'kept';
    history.save();
    editor?.highlight(new Set());
  }

  /** Put the narrative back as it was before turn `i`, as one transaction. */
  async function revert(index: number) {
    const turn = session.turns[index];
    const review = reviews[turn?.id];
    if (!review || review.source !== source || !editor || doc.asking) return;
    await editor.flush();
    const now = editor.storedBlocks();
    if (now.length !== review.after.length || now.some((block, blockIndex) => block.id !== review.after[blockIndex]?.id || !sameContent(block, review.after[blockIndex]))) {
      review.error = 'The narrative changed after this reply. Revert would overwrite newer work; use Undo or edit the affected blocks instead.';
      return;
    }
    const { edits } = diff(now, review.before.map(block => ({ id: block.id, content: block })));
    if (edits.length) {
      const result = await doc.command<{ ok: boolean; error?: string }>({ op: "narrative_edit", source, edits: JSON.stringify(edits), label: "revert model change", actor: "human" });
      if (!result.ok) { review.error = result.error || 'Could not revert the model change.'; return; }
    }
    review.reviewed = 'reverted';
    history.save();
    editor.highlight(new Set());
    await editor.refresh();
  }

  // ---- toolbar ---------------------------------------------------------
  // One row at any panel width: the block kinds, then bold and italic, then
  // the inserts, and a More menu at the end. What does not fit moves into
  // More, the least used first (`OVERFLOW`); More always holds Follow views
  // and the markdown import and export.
  type Kind = { id: string; label: string; icon: Component<any>; title?: string; run: () => void };
  const KINDS: Kind[] = [
    { id: "paragraph", label: "Paragraph", icon: Pilcrow, run: () => editor?.setKind("paragraph") },
    { id: "h1", label: "Heading 1", icon: Heading1, run: () => editor?.setKind("heading", { level: 1 }) },
    { id: "h2", label: "Heading 2", icon: Heading2, run: () => editor?.setKind("heading", { level: 2 }) },
    { id: "bullet", label: "Bulleted item", icon: List, run: () => editor?.setKind("bullet") },
    { id: "ordered", label: "Numbered item", icon: ListOrdered, run: () => editor?.setKind("ordered") },
    { id: "quote", label: "Quote", icon: TextQuote, run: () => editor?.setKind("quote") },
    { id: "code", label: "Code", icon: Code, run: () => editor?.setKind("code") },
  ];
  const MARKS: Kind[] = [
    { id: "bold", label: "Bold", title: "Bold (⌘B)", icon: Bold, run: () => editor?.toggle("strong") },
    { id: "italic", label: "Italic", title: "Italic (⌘I)", icon: Italic, run: () => editor?.toggle("em") },
  ];
  const OVERFLOW = ["code", "quote", "value", "view", "ordered", "bullet", "h2"];
  const BUTTON = 26; // a 24 px icon button and the 2 px gap after it
  const SEP = 9;
  let toolsWidth = $state(0);
  const overflow = $derived.by(() => {
    const out = new Set<string>();
    if (!toolsWidth) return out;
    const room = toolsWidth - 12 - BUTTON; // the row's padding, and More
    const width = () => {
      const blocks = KINDS.filter((k) => !out.has(k.id)).length;
      const inserts = ["view", "value"].filter((k) => !out.has(k)).length;
      return (blocks + MARKS.length + inserts) * BUTTON + SEP * (inserts ? 2 : 1);
    };
    for (const id of OVERFLOW) {
      if (width() <= room) break;
      out.add(id);
    }
    return out;
  });
  let compositions: { id: string; name: string | null; dims: string }[] = $state([]);
  async function loadCompositions() {
    compositions = await doc.command({ op: "compositions" });
  }
  let valueLeaf = $state("");
  let valueDigits = $state("");
  let valueError = $state("");
  let showValue = $state(false);
  async function insertValue() {
    valueError = "";
    let leaf = valueLeaf.trim();
    // `name.facet.field` names the node by name.
    const m = /^([^#.][^.]*)\.([a-z0-9_]+\.[a-z0-9_]+)$/i.exec(leaf);
    if (m) {
      const r = await doc.command<{ ok: boolean; targets: { id: string; name: string }[] }>({ op: "narrative_targets", query: m[1] });
      const hit = r.targets?.find((t) => t.name === m[1]) ?? r.targets?.[0];
      if (!hit) {
        valueError = `nothing is named ${m[1]}`;
        return;
      }
      leaf = `${hit.id}.${m[2]}`;
    }
    if (!/^#[0-9A-Za-z/]+\.[a-z0-9_]+\.[a-z0-9_]+$/i.test(leaf)) {
      valueError = "a leaf: #4.size.w or square.transform2d.x";
      return;
    }
    editor?.insertValue(leaf, null, valueDigits === "" ? null : Number(valueDigits));
    showValue = false;
    valueLeaf = "";
  }

  async function exportMarkdown() {
    await editor?.flush();
    const r = await doc.command<{ ok: boolean; name: string; markdown: string }>({ op: "narrative_markdown", source });
    if (!r.ok) return;
    const url = URL.createObjectURL(new Blob([r.markdown], { type: "text/markdown" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(r.name || "narrative").replace(/[^\w.-]+/g, "-")}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }
  let mdInput: HTMLInputElement | undefined = $state();
  async function importMarkdown(ev: Event) {
    const input = ev.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const name = file.name.replace(/\.(md|markdown|txt)$/i, "");
    const r = await doc.command<{ ok: boolean; error?: string }>({ op: "narrative_create", name, markdown: await file.text(), actor: "human" });
    if (r.ok) {
      await listNarratives();
      open(name);
    }
  }

  // ---- for the driver ---------------------------------------------------
  function api() {
    return {
      source: () => source,
      editor: () => editor,
      mode: (m?: Mode) => (m ? (mode = m) : mode),
      flush: () => editor?.flush(),
      askWith: (xml: string, text = "") => {
        return chat?.submit(text || '(scripted edit)', (instruction, history, turn, live) => performTurn(instruction, history, turn, live, xml));
      },
      turns: () => session.turns.map(turn => ({ ...turn, ok: !!turn.result?.ok, note: turn.result?.note ?? '', ...reviews[turn.id], before: undefined, after: undefined })),
      keep,
      revert,
      status: () => ({ ...status, missing, cursor: { ...cursor }, target: targetLabel, shown: editor?.shown ?? "" }),
    };
  }
</script>

{#snippet viewChoices()}
  <div class="ui-menu-heading">Show a composition</div>
  {#each compositions as c (c.id)}
    <DropdownMenu.Item class="ui-menu-item" onSelect={() => editor?.insertViewBlock(c.name ?? c.id, "")}>
      <span>{c.name ?? c.id}</span><span class="ui-hint">{c.dims}</span>
    </DropdownMenu.Item>
  {/each}
{/snippet}

<script module lang="ts">
  /** Open narrative panels, by view id, for the driver (`__narrative`). */
  const registry = new Map<string, unknown>();
  if (typeof window !== "undefined") (window as any).__narrative = registry;
</script>

<section class="html-view narrative" data-mode={mode}>
  <header class="ui-panel-header bar">
    <Select
        variant="quiet"
        class="pick"
        label="Narrative"
        value={source}
        items={[
          ...(narratives.some((n) => n.name === source || n.id === source) ? [] : [{ value: source, label: source }]),
          ...narratives.map((n) => ({ value: n.name || n.id, label: n.name || n.id })),
        ]}
        onChange={(v) => open(v)}
      />
    <span class="state" title={status.error || `v${status.version}`}>
      {#if status.error && !missing}<span class="err"><TriangleAlert size={12} strokeWidth={1.75} /> {status.error}</span>{:else if status.pending}saving…{:else}saved{/if}
    </span>
    <span class="ui-spacer"></span>
    <!-- The shared segmented look (Segmented.svelte) with each mode's
         `data-mode-button`, which the driver's scripts click. -->
    <ToggleGroup.Root type="single" class="ui-segmented is-inline modes" aria-label="Layout" value={mode} onValueChange={(v) => v && (mode = v as Mode)}>
      {#each MODES as { value: m, label, icon: ModeIcon } (m)}
        <ToggleGroup.Item value={m} class="ui-segment" data-mode-button={m} aria-label={label} title={label}><ModeIcon size={14} strokeWidth={1.75} /></ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
  </header>

  {#if missing}
    <EmptyState icon={BookOpen} title="No narrative named “{source}”" class="missing-narrative">
      Make one, import markdown{narratives.length ? ", or open another" : ""}.
      {#snippet actions()}
        <TextInput placeholder="name" aria-label="New narrative's name" bind:value={newName} onkeydown={(e: KeyboardEvent) => e.key === "Enter" && createNarrative()} />
        <Button variant="primary" onclick={createNarrative}>New narrative</Button>
        <Button variant="outline" onclick={() => mdInput?.click()}><Upload size={14} strokeWidth={1.75} />Import .md…</Button>
        {#each narratives as n (n.id)}<Button small onclick={() => open(n.name || n.id)}>{n.name || n.id}</Button>{/each}
      {/snippet}
    </EmptyState>
  {/if}

  <div class="body" class:hidden={missing}>
    <div class="doc" class:collapsed={mode === "exploration"}>
      {#if mode !== "exploration"}
        <div class="tools" role="toolbar" aria-label="Formatting" bind:clientWidth={toolsWidth}>
          {#each KINDS as k (k.id)}
            {#if !overflow.has(k.id)}<IconButton icon={k.icon} label={k.label} onclick={k.run} />{/if}
          {/each}
          <span class="sep"></span>
          {#each MARKS as k (k.id)}<IconButton icon={k.icon} label={k.label} title={k.title} onclick={k.run} />{/each}
          {#if !overflow.has("view") || !overflow.has("value")}<span class="sep"></span>{/if}
          {#if !overflow.has("view")}
            <DropdownMenu.Root onOpenChange={(o) => o && loadCompositions()}>
              <DropdownMenu.Trigger class="ui-icon-button" aria-label="Insert a view block" title="Insert a view block">
                <SquareCode size={14} strokeWidth={1.75} />
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content class="ui-menu" sideOffset={4} align="start">
                  {@render viewChoices()}
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          {/if}
          {#if !overflow.has("value")}
            <IconButton icon={Variable} label="Insert a live value" pressed={showValue} onPressedChange={(v: boolean) => (showValue = v)} />
          {/if}
          <span class="ui-spacer"></span>
          <DropdownMenu.Root onOpenChange={(o) => o && overflow.has("view") && loadCompositions()}>
            <DropdownMenu.Trigger class="ui-icon-button" aria-label="More" title="More">
              <Ellipsis size={14} strokeWidth={1.75} />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class="ui-menu" sideOffset={4} align="end">
                {#each KINDS.filter((k) => overflow.has(k.id)) as k (k.id)}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={k.run}><k.icon size={14} strokeWidth={1.75} /><span>{k.label}</span></DropdownMenu.Item>
                {/each}
                {#if overflow.has("view")}
                  <DropdownMenu.Sub>
                    <DropdownMenu.SubTrigger class="ui-menu-item"><SquareCode size={14} strokeWidth={1.75} /><span>Insert a view block</span><ChevronRight class="ui-submenu-chevron" size={14} strokeWidth={1.75} /></DropdownMenu.SubTrigger>
                    <DropdownMenu.SubContent class="ui-menu" sideOffset={4}>{@render viewChoices()}</DropdownMenu.SubContent>
                  </DropdownMenu.Sub>
                {/if}
                {#if overflow.has("value")}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => (showValue = !showValue)}><Variable size={14} strokeWidth={1.75} /><span>Insert a live value</span></DropdownMenu.Item>
                {/if}
                {#if overflow.size}<DropdownMenu.Separator class="ui-menu-separator" />{/if}
                <DropdownMenu.CheckboxItem class="ui-menu-item" bind:checked={follow} closeOnSelect={false} title="Show each view block's composition as it scrolls to the top">
                  {#snippet children({ checked })}
                    <span class="ui-menu-check">{#if checked}<Check size={14} strokeWidth={1.75} />{/if}</span><span>Follow views</span><span class="ui-hint">as you scroll</span>
                  {/snippet}
                </DropdownMenu.CheckboxItem>
                <DropdownMenu.Separator class="ui-menu-separator" />
                <DropdownMenu.Item class="ui-menu-item" onSelect={exportMarkdown}><Download size={14} strokeWidth={1.75} /><span>Export as markdown</span></DropdownMenu.Item>
                <DropdownMenu.Item class="ui-menu-item" onSelect={() => mdInput?.click()}><Upload size={14} strokeWidth={1.75} /><span>Import markdown as a new narrative…</span></DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
        {#if showValue}
          <div class="value-form">
            <TextInput class="leaf" aria-label="Leaf" placeholder="#4.size.w or square.transform2d.x" bind:value={valueLeaf} onkeydown={(e: KeyboardEvent) => e.key === "Enter" && insertValue()} />
            <TextInput class="digits" aria-label="Digits" placeholder="digits" bind:value={valueDigits} />
            <Button variant="outline" small onclick={insertValue}>Insert</Button>
            {#if valueError}<span class="err">{valueError}</span>{/if}
          </div>
        {/if}
      {:else}
        <nav class="outline" aria-label="Outline">
          {#each outline as o (o.id)}
            <BitsButton.Root class="outline-item" style="padding-left: {(o.level - 1) * 10 + 6}px" onclick={() => {
              mode = "hybrid";
              setTimeout(() => editor?.reveal(o.id), 0);
            }}>{#if o.view}<SquarePlay size={14} strokeWidth={1.75} />{/if}<span>{o.text || "(untitled)"}</span></BitsButton.Root>
          {/each}
        </nav>
      {/if}
      <div class="scroll" bind:this={scroller} class:hidden={mode === "exploration"}>
        <div class="editor" bind:this={host}></div>
      </div>
    </div>

    <div class="conversation-host" class:compact={mode === 'authoring'}>
      <ChatPanel {session} {history} bind:this={chat} compact={mode === 'authoring'} target={targetLabel}
        targets={[{ value: "cursor", label: cursorLabel }, { value: "conversation", label: "Conversation only" }]}
        targetValue={writeAtCursor ? "cursor" : "conversation"} onTarget={(v) => (writeAtCursor = v === "cursor")} send={performTurn}>
        {#snippet review(turn: ChatTurn)}
          {@const change = reviews[turn.id]}
          {@const index = session.turns.findIndex(item => item.id === turn.id)}
          {#if change?.changed.length}
            <div class="review" data-review={index}>
              <span>Changed {change.changed.filter(id => id !== '(deleted)').length} block{change.changed.length === 1 ? '' : 's'}{change.changed.includes('(deleted)') ? ', deleted some' : ''}</span>
              {#if change.reviewed}<span class="done">{change.reviewed}</span>
              {:else}
                <Button small onclick={() => editor?.reveal(change.changed.find(id => id !== '(deleted)') ?? '')}>Show</Button>
                <Button variant="primary" small onclick={() => keep(index)} data-keep>Keep</Button>
                <Button variant="outline" small onclick={() => revert(index)} data-revert disabled={doc.asking}>Revert</Button>
              {/if}
              {#if change.error}<span class="err" role="alert">{change.error}</span>{/if}
            </div>
          {/if}
        {/snippet}
      </ChatPanel>
    </div>
  </div>
  <input type="file" accept=".md,.markdown,.txt,text/markdown" bind:this={mdInput} hidden onchange={importMarkdown} />

  {#if mention && targets.length}
    <div class="mentions ui-menu" role="listbox" style:left="{mention.left}px" style:top="{mention.bottom + 4}px">
      {#each targets as t, i (t.id)}
        <BitsButton.Root class="ui-menu-item mention" role="option" aria-selected={i === pick} data-highlighted={i === pick ? "" : undefined} onmousedown={(e: MouseEvent) => {
          e.preventDefault();
          choose(i);
        }}>
          <span>{t.name}</span><span class="ui-hint">{t.type}{t.composition ? ` · ${t.composition}` : ""} {t.id}</span>
        </BitsButton.Root>
      {/each}
    </div>
  {/if}
</section>

<style>
  .narrative {
    padding: 0 !important;
    gap: 0 !important;
  }
  .bar :global(.pick) {
    flex: 0 1 auto;
    min-width: 0;
    font-weight: 600;
  }
  /* In a narrow panel the save state gives way before the narrative's name. */
  .state {
    flex: 0 100 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-faint);
    font-size: var(--text-sm);
  }
  .bar :global(.modes) {
    flex: none;
  }
  .err {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--danger);
  }
  .narrative :global(.missing-narrative .ui-empty-actions) {
    flex-wrap: wrap;
    justify-content: center;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .hidden {
    display: none !important;
  }
  .doc {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .narrative[data-mode="hybrid"] .doc {
    flex: 3;
  }
  .doc.collapsed {
    flex: none;
    max-height: 35%;
    overflow: auto;
    border-bottom: 1px solid var(--border);
  }
  /* The formatting toolbar: one row in the panel header's rhythm; what
     does not fit is in its More menu. */
  .tools {
    display: flex;
    align-items: center;
    flex: none;
    gap: 2px;
    height: var(--panel-header-h);
    padding: 0 6px;
    border-bottom: 1px solid var(--border);
    overflow: hidden;
  }
  .tools .sep {
    flex: none;
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 3px;
  }
  .tools .ui-spacer {
    flex: 1;
  }
  .value-form {
    display: flex;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    align-items: center;
  }
  .value-form :global(.leaf) {
    flex: 1;
  }
  .value-form :global(.digits) {
    flex: none;
    width: 70px;
  }
  .outline {
    display: flex;
    flex-direction: column;
    padding: 6px;
  }
  .outline :global(.outline-item) {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--control-h-sm);
    text-align: left;
    border: none;
    background: none;
    padding: 0 6px;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-md);
    cursor: pointer;
    border-radius: var(--radius);
  }
  .outline :global(.outline-item > svg) {
    flex: none;
    color: var(--fg-faint);
  }
  .outline :global(.outline-item:hover) {
    background: var(--hover);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .narrative .editor :global(.ProseMirror) {
    outline: none;
    padding: 12px 18px 40vh;
    font-size: var(--text-lg);
    line-height: 1.55;
    white-space: pre-wrap;
    word-wrap: break-word;
  }
  .narrative .editor :global(.ProseMirror p) {
    margin: 0 0 0.6em;
  }
  .narrative .editor :global(.ProseMirror h1) {
    font-size: 1.5em;
    margin: 0.3em 0 0.5em;
  }
  .narrative .editor :global(.ProseMirror h2) {
    font-size: 1.2em;
    margin: 0.8em 0 0.4em;
  }
  .narrative .editor :global(.ProseMirror h3) {
    font-size: 1.05em;
  }
  .narrative .editor :global(.nb-bullet),
  .narrative .editor :global(.nb-ordered) {
    position: relative;
    padding-left: 1.4em;
    margin: 0 0 0.25em !important;
  }
  .narrative .editor :global(.nb-bullet)::before {
    content: "•";
    position: absolute;
    left: 0.4em;
  }
  .narrative .editor :global(.nb-ordered)::before {
    content: "–";
    position: absolute;
    left: 0.3em;
    color: var(--fg-faint);
  }
  .narrative .editor :global(blockquote) {
    margin: 0 0 0.6em;
    padding-left: 10px;
    border-left: 3px solid var(--border-strong);
    color: var(--fg-muted);
  }
  .narrative .editor :global(pre) {
    background: var(--bg-sunken);
    padding: 8px;
    border-radius: var(--radius);
    font-size: var(--text-md);
  }
  .narrative .editor :global(.nb-mention) {
    color: var(--accent);
    border-bottom: 1px dotted var(--accent);
    cursor: pointer;
  }
  .narrative .editor :global(.nb-mention.nb-chip) {
    background: var(--accent-soft);
    border-bottom: none;
    border-radius: var(--radius-sm);
    padding: 0 3px;
  }
  .narrative .editor :global(.nb-mention:hover) {
    background: var(--accent-soft);
  }
  .narrative .editor :global(.nb-stale) {
    color: var(--fg-faint);
    text-decoration: line-through;
    border-bottom-color: var(--fg-faint);
  }
  .narrative .editor :global(.nb-value) {
    font-variant-numeric: tabular-nums;
    background: var(--bg-sunken);
    border-radius: var(--radius-sm);
    padding: 0 4px;
    font-family: var(--font-mono);
    font-size: 0.92em;
  }
  .narrative .editor :global(.nb-view) {
    margin: 0.8em 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .narrative .editor :global(.nb-view.nb-view-shown) {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent-soft);
  }
  /* A view block's header: the disclosure look (`.ui-disclosure-trigger`,
     its chevron from editor.ts) across the block's top. */
  .narrative .editor :global(.nb-view-header) {
    display: flex;
    width: 100%;
    min-height: var(--control-h);
    margin: 0;
    padding: 0 10px 0 6px;
    border-radius: 0;
    background: var(--bg-sunken);
    text-align: left;
  }
  .narrative .editor :global(.nb-view-header:hover) {
    background: var(--hover);
  }
  .narrative .editor :global(.nb-view-caption) {
    padding: 6px 10px;
    min-height: 1.4em;
    color: var(--fg-muted);
    font-style: italic;
  }
  .narrative .editor :global(.nb-changed) {
    background: color-mix(in srgb, var(--accent-soft) 60%, transparent);
    box-shadow: -3px 0 0 var(--accent);
  }
  .conversation-host {
    flex: 2;
    min-height: 0;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
  }
  .conversation-host.compact {
    flex: none;
    max-height: 280px;
  }
  .narrative[data-mode="exploration"] .conversation-host {
    flex: 1;
  }
  .review {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    font-size: var(--text-md);
  }
  .review .done {
    color: var(--fg-faint);
  }
  /* The # menu: the shared menu surface (`.ui-menu`), at the caret. */
  .mentions {
    position: fixed;
    min-width: 220px;
    max-height: 240px;
    display: flex;
    flex-direction: column;
    pointer-events: auto;
  }
  .mentions :global(.mention) {
    width: 100%;
    border: none;
    background: none;
    color: inherit;
    text-align: left;
    font: inherit;
  }
</style>
