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
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";
  import { showComposition } from "../workspace";
  import { NarrativeEditor, type CursorInfo, type MentionQuery } from "../../narrative/editor";
  import { diff, sameContent, type StoredBlock } from "../../narrative/convert";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

  const doc = controllerManager.docClient;
  const active = getContext<() => boolean>("iron-view-active") ?? (() => true);
  type Mode = "authoring" | "hybrid" | "exploration";
  // svelte-ignore state_referenced_locally
  const modeKey = `iron.narrative.mode.${panel ?? spec.source}`;
  let mode: Mode = $state((() => {
    try {
      return (localStorage.getItem(modeKey) as Mode) || "authoring";
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
  interface Turn {
    instruction: string;
    target: string;
    ok: boolean;
    note: string;
    error: string;
    /** What the narrative held before, and which blocks the turn changed. */
    before: StoredBlock[] | null;
    changed: string[];
    reviewed: "" | "kept" | "reverted";
  }
  let turns: Turn[] = $state([]);
  let instruction = $state("");
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

  async function send(xmlForTest?: string) {
    const text = instruction.trim();
    if (!text && !xmlForTest) return;
    await editor?.flush();
    const before = editor?.storedBlocks() ?? null;
    const turn: Turn = { instruction: text || "(scripted edit)", target: targetLabel, ok: false, note: "", error: "", before, changed: [], reviewed: "" };
    turns = [...turns, turn];
    instruction = "";
    try {
      if (xmlForTest !== undefined) {
        const r = await doc.edit(xmlForTest, `model: ${text.slice(0, 60)}`);
        turn.ok = !!r?.ok;
        turn.error = r?.ok ? "" : (r?.failure ?? "not applied");
      } else {
        const r = await doc.ask(contextFor(text));
        turn.ok = !!r?.ok;
        turn.note = r?.note ?? "";
        turn.error = r?.error ?? (r?.ok ? "" : (r?.outcome?.failure ?? ""));
      }
    } catch (e) {
      turn.error = String(e);
    }
    await editor?.refresh();
    const after = editor?.storedBlocks() ?? [];
    if (before) {
      const was = new Map(before.map((b) => [b.id, b]));
      turn.changed = after.filter((b) => !was.has(b.id) || !sameContent(was.get(b.id)!, b)).map((b) => b.id);
      if (before.some((b) => !after.find((a) => a.id === b.id))) turn.changed.push("(deleted)");
    }
    turns = [...turns.slice(0, -1), { ...turn }];
    editor?.highlight(new Set(turn.changed));
    if (turn.changed[0] && turn.changed[0] !== "(deleted)") editor?.reveal(turn.changed[0]);
  }

  function keep(i: number) {
    turns[i] = { ...turns[i], reviewed: "kept" };
    editor?.highlight(new Set());
  }

  /** Put the narrative back as it was before turn `i`, as one transaction. */
  async function revert(i: number) {
    const t = turns[i];
    if (!t.before || !editor) return;
    await editor.flush();
    const now = editor.storedBlocks();
    const { edits } = diff(now, t.before.map((b) => ({ id: b.id, content: b })));
    if (edits.length) await doc.command({ op: "narrative_edit", source, edits: JSON.stringify(edits), label: "revert model change", actor: "human" });
    turns[i] = { ...t, reviewed: "reverted" };
    editor.highlight(new Set());
    await editor.refresh();
  }

  // ---- toolbar ---------------------------------------------------------
  let compositions: { id: string; name: string | null; dims: string }[] = $state([]);
  async function loadCompositions() {
    compositions = await doc.command({ op: "compositions" });
  }
  let viewPick = $state("");
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
        instruction = text;
        return send(xml);
      },
      turns: () => turns.map((t) => ({ ...t, before: undefined })),
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
    <select class="pick" aria-label="Narrative" value={source} onfocus={listNarratives} onchange={(e) => open((e.currentTarget as HTMLSelectElement).value)}>
      {#if !narratives.some((n) => n.name === source || n.id === source)}<option value={source}>{source}</option>{/if}
      {#each narratives as n (n.id)}<option value={n.name || n.id}>{n.name || n.id}</option>{/each}
    </select>
    <div class="modes" role="radiogroup" aria-label="Layout">
      {#each [["authoring", "Authoring"], ["hybrid", "Hybrid"], ["exploration", "Exploration"]] as [m, label] (m)}
        <button role="radio" aria-checked={mode === m} class:on={mode === m} data-mode-button={m} onclick={() => (mode = m as Mode)}>{label}</button>
      {/each}
    </div>
    <span class="state" title={status.error || `v${status.version}`}>
      {#if status.error && !missing}<span class="err">⚠ {status.error}</span>{:else if status.pending}saving…{:else}saved{/if}
    </span>
  </header>

  {#if missing}
    <div class="empty">
      <p>No narrative named <strong>{source}</strong> in this scope.</p>
      <div class="row">
        <input placeholder="name" bind:value={newName} onkeydown={(e) => e.key === "Enter" && createNarrative()} />
        <button onclick={createNarrative}>New narrative</button>
        <button onclick={() => mdInput?.click()}>Import .md…</button>
      </div>
      {#if narratives.length}
        <p class="hint">Or open one: {#each narratives as n, i (n.id)}{i ? ", " : ""}<button class="link" onclick={() => open(n.name || n.id)}>{n.name || n.id}</button>{/each}</p>
      {/if}
    </div>
  {/if}

  <div class="body" class:hidden={missing}>
    <div class="doc" class:collapsed={mode === "exploration"}>
      {#if mode !== "exploration"}
        <div class="tools" role="toolbar" aria-label="Formatting">
          <button title="Paragraph" onclick={() => editor?.setKind("paragraph")}>¶</button>
          <button title="Heading 1" onclick={() => editor?.setKind("heading", { level: 1 })}>H1</button>
          <button title="Heading 2" onclick={() => editor?.setKind("heading", { level: 2 })}>H2</button>
          <button title="Bulleted item" onclick={() => editor?.setKind("bullet")}>•</button>
          <button title="Numbered item" onclick={() => editor?.setKind("ordered")}>1.</button>
          <button title="Quote" onclick={() => editor?.setKind("quote")}>❝</button>
          <button title="Code" onclick={() => editor?.setKind("code")}>{"{}"}</button>
          <span class="sep"></span>
          <button title="Bold (⌘B)" onclick={() => editor?.toggle("strong")}><b>B</b></button>
          <button title="Italic (⌘I)" onclick={() => editor?.toggle("em")}><i>I</i></button>
          <span class="sep"></span>
          <select title="Insert a view block" aria-label="Insert a view block" value={viewPick} onfocus={loadCompositions} onchange={(e) => {
            const v = (e.currentTarget as HTMLSelectElement).value;
            if (v) editor?.insertViewBlock(v, "");
            viewPick = "";
            (e.currentTarget as HTMLSelectElement).value = "";
          }}>
            <option value="">View…</option>
            {#each compositions as c (c.id)}<option value={c.name ?? c.id}>{c.name ?? c.id} ({c.dims})</option>{/each}
          </select>
          <button title="Insert a live value" onclick={() => (showValue = !showValue)}>= value</button>
          <label class="follow" title="Show each view block's composition as it scrolls to the top">
            <input type="checkbox" bind:checked={follow} /> follow views
          </label>
          <span class="spacer"></span>
          <button title="Export as markdown" onclick={exportMarkdown}>.md ↓</button>
          <button title="Import markdown as a new narrative" onclick={() => mdInput?.click()}>.md ↑</button>
        </div>
        {#if showValue}
          <div class="value-form">
            <input placeholder="#4.size.w or square.transform2d.x" bind:value={valueLeaf} onkeydown={(e) => e.key === "Enter" && insertValue()} />
            <input class="digits" placeholder="digits" bind:value={valueDigits} />
            <button onclick={insertValue}>Insert</button>
            {#if valueError}<span class="err">{valueError}</span>{/if}
          </div>
        {/if}
      {:else}
        <nav class="outline" aria-label="Outline">
          {#each outline as o (o.id)}
            <button style:padding-left="{(o.level - 1) * 10 + 6}px" onclick={() => {
              mode = "hybrid";
              setTimeout(() => editor?.reveal(o.id), 0);
            }}>{o.text || "(untitled)"}</button>
          {/each}
        </nav>
      {/if}
      <div class="scroll" bind:this={scroller} class:hidden={mode === "exploration"}>
        <div class="editor" bind:this={host}></div>
      </div>
    </div>

    {#if mode !== "authoring" || turns.length}
      <div class="conversation" class:compact={mode === "authoring"}>
        {#each turns as t, i (i)}
          {#if mode !== "authoring" || i === turns.length - 1}
            <div class="turn" class:failed={!t.ok}>
              <p class="you">{t.instruction}</p>
              <p class="meta">{t.target}{t.ok ? "" : " · not applied"}</p>
              {#if t.note}<p class="note">{t.note}</p>{/if}
              {#if t.error}<p class="err">{t.error}</p>{/if}
              {#if t.changed.length}
                <div class="review" data-review={i}>
                  <span>Changed {t.changed.filter((c) => c !== "(deleted)").length} block{t.changed.length === 1 ? "" : "s"}{t.changed.includes("(deleted)") ? ", deleted some" : ""}</span>
                  {#if t.reviewed}
                    <span class="done">{t.reviewed}</span>
                  {:else}
                    <button onclick={() => editor?.reveal(t.changed[0])}>Show</button>
                    <button onclick={() => keep(i)} data-keep>Keep</button>
                    <button onclick={() => revert(i)} data-revert>Revert</button>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}

    <div class="composer">
      <div class="target">
        <button class="toggle" title="Where the model writes" onclick={() => (writeAtCursor = !writeAtCursor)}>{targetLabel}</button>
        {#if !doc.apiKey}<input class="key" type="password" placeholder="Anthropic API key" onchange={(e) => doc.setApiKey((e.currentTarget as HTMLInputElement).value)} />{/if}
      </div>
      <textarea
        rows={mode === "authoring" ? 2 : 3}
        placeholder={writeAtCursor ? "Ask the model to write here… (⌘↵)" : "Talk to the model… (⌘↵)"}
        bind:value={instruction}
        onkeydown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void send();
        }}
      ></textarea>
      <button class="send" onclick={() => send()} disabled={doc.asking || !doc.apiKey || !instruction.trim()}>{doc.asking ? "Asking…" : "Ask"}</button>
    </div>
  </div>
  <input type="file" accept=".md,.markdown,.txt,text/markdown" bind:this={mdInput} hidden onchange={importMarkdown} />

  {#if mention && targets.length}
    <div class="mentions" role="listbox" style:left="{mention.left}px" style:top="{mention.bottom + 4}px">
      {#each targets as t, i (t.id)}
        <button role="option" aria-selected={i === pick} class:on={i === pick} onmousedown={(e) => {
          e.preventDefault();
          choose(i);
        }}>
          <span>{t.name}</span><span class="hint">{t.type}{t.composition ? ` · ${t.composition}` : ""} {t.id}</span>
        </button>
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
    max-width: 160px;
    font: inherit;
  }
  .modes {
    display: flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .modes button {
    border: none;
    background: transparent;
    padding: 2px 8px;
    font-size: 12px;
    color: var(--fg-muted);
    cursor: pointer;
  }
  .modes button.on {
    background: var(--accent-soft);
    color: var(--fg);
  }
  .state {
    margin-left: auto;
    color: var(--fg-faint);
    font-size: 11px;
  }
  .err {
    color: var(--danger);
  }
  .empty {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0;
    cursor: pointer;
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
  .tools button,
  .tools select {
    font: inherit;
    font-size: 12px;
    padding: 1px 6px;
    border: 1px solid transparent;
    border-radius: 4px;
    background: transparent;
    color: var(--fg);
    cursor: pointer;
  }
  .tools button:hover {
    background: var(--hover);
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
    gap: 3px;
  }
  .value-form {
    display: flex;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    align-items: center;
  }
  .value-form input {
    flex: 1;
    font: inherit;
  }
  .value-form .digits {
    flex: none;
    width: 60px;
  }
  .outline {
    display: flex;
    flex-direction: column;
    padding: 6px;
  }
  .outline button {
    text-align: left;
    border: none;
    background: none;
    padding: 3px 6px;
    color: var(--fg);
    cursor: pointer;
    border-radius: 4px;
  }
  .outline button:hover {
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
  .conversation {
    flex: 2;
    min-height: 0;
    overflow: auto;
    border-top: 1px solid var(--border);
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .conversation.compact {
    flex: none;
    max-height: 140px;
  }
  .narrative[data-mode="exploration"] .conversation {
    flex: 1;
  }
  .turn {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .turn .you {
    font-weight: 500;
  }
  .turn .meta {
    font-size: 11px;
    color: var(--fg-faint);
  }
  .turn .note {
    white-space: pre-wrap;
    color: var(--fg-muted);
  }
  .review {
    display: flex;
    gap: 6px;
    align-items: center;
    font-size: 12px;
  }
  .review .done {
    color: var(--fg-faint);
  }
  .composer {
    flex: none;
    border-top: 1px solid var(--border);
    padding: 6px 10px 8px;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px 6px;
  }
  .composer .target {
    grid-column: 1 / -1;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .composer .toggle {
    border: none;
    background: var(--accent-soft);
    border-radius: 10px;
    padding: 1px 8px;
    font-size: 11px;
    color: var(--fg);
    cursor: pointer;
  }
  .composer .key {
    flex: 1;
    font-size: 11px;
  }
  .composer textarea {
    font: inherit;
    font-size: 12px;
    resize: none;
  }
  .composer .send {
    align-self: end;
  }
  .mentions {
    position: fixed;
    z-index: 1000;
    min-width: 220px;
    max-height: 240px;
    overflow: auto;
    padding: 4px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: var(--menu-shadow);
    display: flex;
    flex-direction: column;
    pointer-events: auto;
  }
  .mentions button {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    border: none;
    background: none;
    padding: 4px 8px;
    border-radius: 5px;
    text-align: left;
    color: var(--fg);
    cursor: pointer;
  }
  .mentions button.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .mentions .hint {
    font-size: 11px;
    opacity: 0.7;
  }
</style>
