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
  import { Button as BitsButton, DropdownMenu, Switch, ToggleGroup } from "bits-ui";
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
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import TextInput from "../../lib/ui/TextInput.svelte";
  import ChatPanel from '../../chat/ChatPanel.svelte';
  import { chatSession } from '../../chat/sessions.svelte';
  import { narrativeMode, type ChatHandle, type ChatTurn, type NarrativeMode as Mode } from '../../chat/conversation';
  import type { ConversationMessage } from '../../model/conversation';
  import type { AskResult } from '../../model/agent';
  import Select from "../../lib/ui/Select.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";
  import { showComposition } from "../workspace";
  import { NarrativeEditor, type CursorInfo, type MentionQuery } from "../../narrative/editor";
  import { diff, sameContent, type StoredBlock } from "../../narrative/convert";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

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
  let outline: { id: string; text: string; level: number }[] = $state([]);
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
    const out: { id: string; text: string; level: number }[] = [];
    e.view.state.doc.forEach((n) => {
      if (n.type.name === "heading" || n.type.name === "view") out.push({ id: n.attrs.id, text: n.type.name === "view" ? `▸ ${n.attrs.show}` : n.textContent, level: n.type.name === "heading" ? n.attrs.level : 3 });
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
    observer = new IntersectionObserver(
      (entries) => {
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
  const session = $derived(chatSession(`${doc.generation}:${doc.scope.current?.id ?? 'demo'}:narrative:${source}`));
  const reviews = $derived(session.annotations as Record<string, NarrativeReview>);
  let chat: ChatHandle | undefined = $state();
  let writeAtCursor = $state(true);
  $effect(() => {
    // Each mode's default target (§4.1): Authoring writes at the cursor,
    // Exploration talks only.
    writeAtCursor = mode !== "exploration";
  });
  const targetLabel = $derived(
    writeAtCursor
      ? cursor.selection
        ? `Edits the selection${cursor.heading ? ` in “${cursor.heading}”` : ""}`
        : `Writes at the cursor${cursor.heading ? `, in “${cursor.heading}”` : ""}`
      : "Conversation only",
  );

  /** The request with where the author is: a hint, not a rule (§4.2). */
  function contextFor(text: string): string {
    if (!writeAtCursor) return `${text}\n\n(The author is talking, not writing: change the narrative "${source}" only if asked to.)`;
    const at = cursor.block ? ` after block [${cursor.block}]` : " at its end";
    const sel = cursor.selection ? `\nThe author has selected: "${cursor.selection.slice(0, 400)}". Keep the edit to the selection unless the request names somewhere else.` : "";
    return `${text}\n\n(The author's cursor is in the narrative "${source}"${at}${cursor.heading ? `, under the heading "${cursor.heading}"` : ""}. Insert new text there with <write narrative="${source}" after="${cursor.block ?? "end"}">; edits elsewhere are allowed when the request implies them.)${sel}`;
  }

  async function performTurn(text: string, history: ConversationMessage[], turn: ChatTurn, xmlForTest?: string): Promise<AskResult> {
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
      result = await doc.ask(instruction, { history, allowTextReply: true });
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
    editor.highlight(new Set());
    await editor.refresh();
  }

  // ---- toolbar ---------------------------------------------------------
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
        return chat?.submit(text || '(scripted edit)', (instruction, history, turn) => performTurn(instruction, history, turn, xml));
      },
      turns: () => session.turns.map(turn => ({ ...turn, ok: !!turn.result?.ok, note: turn.result?.note ?? '', ...reviews[turn.id], before: undefined, after: undefined })),
      keep,
      revert,
      status: () => ({ ...status, missing, cursor: { ...cursor }, target: targetLabel, shown: editor?.shown ?? "" }),
    };
  }
</script>

<script module lang="ts">
  /** Open narrative panels, by view id, for the driver (`__narrative`). */
  const registry = new Map<string, unknown>();
  if (typeof window !== "undefined") (window as any).__narrative = registry;
</script>

<section class="html-view narrative" data-mode={mode}>
  <header class="bar">
    <div class="pick">
      <Select
        label="Narrative"
        value={source}
        items={[
          ...(narratives.some((n) => n.name === source || n.id === source) ? [] : [{ value: source, label: source }]),
          ...narratives.map((n) => ({ value: n.name || n.id, label: n.name || n.id })),
        ]}
        onChange={(v) => open(v)}
      />
    </div>
    <ToggleGroup.Root type="single" class="modes" aria-label="Layout" value={mode} onValueChange={(v) => v && (mode = v as Mode)}>
      {#each [["authoring", "Authoring"], ["hybrid", "Hybrid"], ["exploration", "Exploration"]] as [m, label] (m)}
        <ToggleGroup.Item value={m} class="ui-button is-small mode" data-mode-button={m}>{label}</ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
    <span class="state" title={status.error || `v${status.version}`}>
      {#if status.error && !missing}<span class="err"><TriangleAlert size={12} strokeWidth={1.75} /> {status.error}</span>{:else if status.pending}saving…{:else}saved{/if}
    </span>
  </header>

  {#if missing}
    <div class="empty">
      <p>No narrative named <strong>{source}</strong> in this scope.</p>
      <div class="row">
        <TextInput placeholder="name" aria-label="New narrative's name" bind:value={newName} onkeydown={(e: KeyboardEvent) => e.key === "Enter" && createNarrative()} />
        <Button variant="primary" onclick={createNarrative}>New narrative</Button>
        <Button variant="outline" onclick={() => mdInput?.click()}><Upload size={13} strokeWidth={1.75} />Import .md…</Button>
      </div>
      {#if narratives.length}
        <div class="hint">Or open one: {#each narratives as n (n.id)}<Button small onclick={() => open(n.name || n.id)}>{n.name || n.id}</Button>{/each}</div>
      {/if}
    </div>
  {/if}

  <div class="body" class:hidden={missing}>
    <div class="doc" class:collapsed={mode === "exploration"}>
      {#if mode !== "exploration"}
        <div class="tools" role="toolbar" aria-label="Formatting">
          <IconButton icon={Pilcrow} label="Paragraph" onclick={() => editor?.setKind("paragraph")} />
          <IconButton icon={Heading1} label="Heading 1" onclick={() => editor?.setKind("heading", { level: 1 })} />
          <IconButton icon={Heading2} label="Heading 2" onclick={() => editor?.setKind("heading", { level: 2 })} />
          <IconButton icon={List} label="Bulleted item" onclick={() => editor?.setKind("bullet")} />
          <IconButton icon={ListOrdered} label="Numbered item" onclick={() => editor?.setKind("ordered")} />
          <IconButton icon={TextQuote} label="Quote" onclick={() => editor?.setKind("quote")} />
          <IconButton icon={Code} label="Code" onclick={() => editor?.setKind("code")} />
          <span class="sep"></span>
          <IconButton icon={Bold} label="Bold" title="Bold (⌘B)" onclick={() => editor?.toggle("strong")} />
          <IconButton icon={Italic} label="Italic" title="Italic (⌘I)" onclick={() => editor?.toggle("em")} />
          <span class="sep"></span>
          <DropdownMenu.Root onOpenChange={(o) => o && loadCompositions()}>
            <DropdownMenu.Trigger class="property-icon" aria-label="Insert a view block" title="Insert a view block">
              <SquareCode size={14} strokeWidth={1.75} />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content class="ui-menu" sideOffset={4} align="start">
                <div class="ui-menu-heading">Show a composition</div>
                {#each compositions as c (c.id)}
                  <DropdownMenu.Item class="ui-menu-item" onSelect={() => editor?.insertViewBlock(c.name ?? c.id, "")}>
                    <span>{c.name ?? c.id}</span><span class="ui-hint">{c.dims}</span>
                  </DropdownMenu.Item>
                {/each}
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
          <IconButton icon={Variable} label="Insert a live value" pressed={showValue} onPressedChange={(v: boolean) => (showValue = v)} />
          <label class="follow" title="Show each view block's composition as it scrolls to the top">
            <Switch.Root class="ui-switch" bind:checked={follow} aria-label="Follow views"><Switch.Thumb class="ui-switch-thumb" /></Switch.Root>
            Follow views
          </label>
          <span class="spacer"></span>
          <IconButton icon={Download} label="Export as markdown" onclick={exportMarkdown} />
          <IconButton icon={Upload} label="Import markdown as a new narrative" onclick={() => mdInput?.click()} />
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
            }}>{o.text || "(untitled)"}</BitsButton.Root>
          {/each}
        </nav>
      {/if}
      <div class="scroll" bind:this={scroller} class:hidden={mode === "exploration"}>
        <div class="editor" bind:this={host}></div>
      </div>
    </div>

    <div class="conversation-host" class:compact={mode === 'authoring'}>
      <ChatPanel {session} bind:this={chat} compact={mode === 'authoring'} target={targetLabel} send={performTurn}>
        {#snippet targetControl()}
          <Button variant="outline" small title="Where the model writes: click to switch" onclick={() => writeAtCursor = !writeAtCursor}>{targetLabel}</Button>
        {/snippet}
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
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .pick {
    width: 160px;
    min-width: 0;
  }
  .bar :global(.modes) {
    display: flex;
    gap: 2px;
  }
  .bar :global(.mode[data-state="on"]) {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .state {
    margin-left: auto;
    color: var(--fg-faint);
    font-size: 11px;
  }
  .err {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--danger);
  }
  .empty {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .empty .row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .empty .hint {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    color: var(--fg-muted);
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
  .tools {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  .tools .sep {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 4px;
  }
  .tools .spacer {
    flex: 1;
  }
  .follow {
    font-size: 11px;
    color: var(--fg-muted);
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: 6px;
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
    text-align: left;
    border: none;
    background: none;
    padding: 3px 6px;
    color: var(--fg);
    cursor: pointer;
    border-radius: 4px;
  }
  .outline :global(.outline-item:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
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
    font-size: 14px;
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
    border-radius: 6px;
    font-size: 12px;
  }
  .narrative .editor :global(.nb-mention) {
    color: var(--accent);
    border-bottom: 1px dotted var(--accent);
    cursor: pointer;
  }
  .narrative .editor :global(.nb-mention.nb-chip) {
    background: var(--accent-soft);
    border-bottom: none;
    border-radius: 4px;
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
    border-radius: 4px;
    padding: 0 4px;
    font-family: "JetBrains Mono", ui-monospace, monospace;
    font-size: 0.92em;
  }
  .narrative .editor :global(.nb-view) {
    margin: 0.8em 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .narrative .editor :global(.nb-view.nb-view-shown) {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent-soft);
  }
  .narrative .editor :global(.nb-view-header) {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: var(--bg-sunken);
    padding: 5px 10px;
    font: inherit;
    font-size: 12px;
    color: var(--fg-muted);
    cursor: pointer;
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
    font-size: 12px;
  }
  .review .done {
    color: var(--fg-faint);
  }
  .mentions {
    position: fixed;
    z-index: 1000;
    min-width: 220px;
    max-height: 240px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    pointer-events: auto;
  }
  .mentions :global(.mention) {
    justify-content: space-between;
    gap: 12px;
    border: none;
    background: none;
    width: 100%;
    text-align: left;
    font: inherit;
  }
</style>
