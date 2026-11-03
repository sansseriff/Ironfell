/**
 * The narrative editor (plans/specs/narrative-panel.md §3): ProseMirror over
 * one narrative, kept in step with the store in both directions.
 *
 * - **Editor → store.** Edits mark the editor dirty; after a 400 ms pause
 *   the editor's blocks are diffed against the last store read and sent as
 *   one `narrative_edit` (one transaction, one undo step). A block the editor
 *   made carries a temporary id (`~n`) until the store's id comes back.
 * - **Store → editor.** On every document change the narrative is read
 *   again and changed blocks are replaced in place, keeping the cursor; a
 *   change of order or count replaces the document, keeping the cursor by
 *   block and offset. While local edits are unsent, the read waits.
 * - **Undo** is the store's: Cmd+Z sends what is pending, then undoes.
 *
 * Mentions (`#` opens a menu the panel draws), live values (atoms whose text
 * comes from `narrative_values`) and view blocks (a header that shows the
 * composition) are node views and decorations here; the panel owns the
 * menus and the layout.
 */
import { EditorState, Plugin, TextSelection, type Command, type Transaction } from 'prosemirror-state';
import { Decoration, DecorationSet, EditorView, type NodeView } from 'prosemirror-view';
import { keymap } from 'prosemirror-keymap';
import { baseKeymap, setBlockType, toggleMark } from 'prosemirror-commands';
import { InputRule, inputRules, textblockTypeInputRule } from 'prosemirror-inputrules';
import type { Node as PMNode, NodeType } from 'prosemirror-model';
import { schema, nodeName } from './schema';
import { blockOf, diff, isTemp, nodeOf, sameContent, valueExpr, type Edit, type StoredBlock } from './convert';

export interface ReadReply {
  ok: boolean;
  error?: string;
  id?: string;
  name?: string;
  version?: number;
  blocks?: StoredBlock[];
}

export interface NarrativeHost {
  read(source: string): Promise<ReadReply>;
  edit(source: string, edits: Edit[], label: string): Promise<{ ok: boolean; version?: number; minted?: Record<string, string>; error?: string }>;
  values(source: string): Promise<{ ok: boolean; values?: Record<string, string>; errors?: Record<string, string> }>;
  undo(): void;
  redo(): void;
  probe(node: string, action: 'hover' | 'unhover' | 'select'): void;
  showView(show: string, focus: string): void;
}

/** The `#…` being typed, for the mention menu: replace `from..to`. */
export interface MentionQuery {
  from: number;
  to: number;
  query: string;
  left: number;
  bottom: number;
}

/** Where the cursor is, for the composer ("writes at cursor, §3"). */
export interface CursorInfo {
  block: string | null;
  index: number;
  heading: string;
  selection: string;
}

const PAUSE_MS = 400;

export class NarrativeEditor {
  readonly view: EditorView;
  private stored: StoredBlock[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private inflight: Promise<void> | null = null;
  private dirty = false;
  private wantRefresh = false;
  private temp = 0;
  private values = new Map<string, string>();
  private valueViews = new Set<ValueView>();
  private destroyed = false;
  /** Mention targets' names and whether they are gone, from the last read. */
  labels = new Map<string, { label: string; stale: boolean }>();
  /** The composition a view block last showed, for its highlight. */
  shown = '';
  /** Blocks a model turn changed, marked until reviewed. */
  private changed = new Set<string>();
  onMention: (q: MentionQuery | null) => void = () => {};
  /** Keys while the mention menu is open: return true to take the key. */
  mentionKeys: ((e: KeyboardEvent) => boolean) | null = null;
  onCursor: (c: CursorInfo) => void = () => {};
  onStatus: (s: { pending: boolean; error: string; version: number; blocks: number }) => void = () => {};
  private version = 0;
  private error = '';

  constructor(mount: HTMLElement, private host: NarrativeHost, public source: string) {
    const editor = this;
    const state = EditorState.create({
      doc: schema.nodes.doc.create(null, [schema.nodes.paragraph.create()]),
      plugins: [
        // The mention menu's keys come before every keymap (Enter picks).
        new Plugin({ props: { handleKeyDown: (_v, e) => !!this.mentionKeys?.(e) } }),
        inputRules({ rules: rules() }),
        keymap({
          'Mod-z': () => (this.historyStep('undo'), true),
          'Mod-Shift-z': () => (this.historyStep('redo'), true),
          'Mod-y': () => (this.historyStep('redo'), true),
          'Mod-b': toggleMark(schema.marks.strong),
          'Mod-i': toggleMark(schema.marks.em),
          'Mod-e': toggleMark(schema.marks.code),
          Backspace: toParagraphAtStart,
        }),
        keymap(baseKeymap),
        tempIds(() => `~${++this.temp}`),
        new Plugin({
          props: {
            decorations: (s) => mentionDecorations(s.doc, this.labels, this.shown, this.changed),
            handleDOMEvents: {
              mouseover: (_v, e) => {
                const t = (e.target as HTMLElement).closest?.('.nb-mention') as HTMLElement | null;
                if (t?.dataset.target) this.host.probe(t.dataset.target, 'hover');
                return false;
              },
              mouseout: (_v, e) => {
                const t = (e.target as HTMLElement).closest?.('.nb-mention') as HTMLElement | null;
                if (t) this.host.probe('', 'unhover');
                return false;
              },
              click: (_v, e) => {
                const t = (e.target as HTMLElement).closest?.('.nb-mention') as HTMLElement | null;
                if (t?.dataset.target && (e.metaKey || e.ctrlKey || e.altKey)) this.host.probe(t.dataset.target, 'select');
                return false;
              },
            },
          },
        }),
      ],
    });
    this.view = new EditorView(mount, {
      state,
      nodeViews: {
        value: (node) => new ValueView(node, editor),
        view: (node, view, getPos) => new ViewBlockView(node, view, getPos as () => number, editor),
      },
      dispatchTransaction(tr: Transaction) {
        const view = this as unknown as EditorView;
        view.updateState(view.state.apply(tr));
        if (tr.docChanged && !tr.getMeta('fromStore')) editor.touched();
        editor.afterUpdate();
      },
    });
  }

  destroy() {
    this.destroyed = true;
    if (this.timer) clearTimeout(this.timer);
    this.view.destroy();
  }

  // ---- store → editor ------------------------------------------------

  /** Read the narrative again and show what changed (on documentChanged). */
  async refresh(): Promise<void> {
    if (this.destroyed) return;
    if (this.dirty || this.inflight) {
      this.wantRefresh = true;
      return;
    }
    let r: ReadReply;
    try {
      r = await this.host.read(this.source);
    } catch {
      // No engine session yet (or a restart): the next document change reads.
      return;
    }
    if (this.destroyed) return;
    if (this.dirty || this.inflight) {
      this.wantRefresh = true;
      return;
    }
    if (!r.ok || !r.blocks) {
      this.error = r.error ?? 'unreadable';
      this.status();
      return;
    }
    this.error = '';
    this.version = r.version ?? 0;
    this.stored = r.blocks;
    this.labels.clear();
    for (const b of r.blocks) for (const m of b.marks) if (m.target && m.label !== undefined) this.labels.set(m.target, { label: m.label, stale: !!m.stale });
    this.reconcile(r.blocks);
    this.status();
    await this.refreshValues();
  }

  private reconcile(blocks: StoredBlock[]) {
    const doc = this.view.state.doc;
    const tr = this.view.state.tr;
    const here: { node: PMNode; pos: number }[] = [];
    doc.forEach((node, pos) => here.push({ node, pos }));
    const same = here.length === blocks.length && here.every((h, i) => h.node.attrs.id === blocks[i].id);
    if (same) {
      for (let i = blocks.length - 1; i >= 0; i--) {
        const { node, pos } = here[i];
        if (!sameContent(blockOf(node), blocks[i]) || node.type.name !== nodeName(blocks[i].kind)) {
          tr.replaceWith(pos, pos + node.nodeSize, nodeOf(schema, blocks[i]));
        }
      }
    } else {
      const head = this.view.state.selection.$head;
      const at = head.depth > 0 ? doc.child(head.index(0)).attrs.id : null;
      const offset = head.parentOffset;
      const nodes = blocks.length ? blocks.map((b) => nodeOf(schema, b)) : [schema.nodes.paragraph.create()];
      tr.replaceWith(0, doc.content.size, nodes);
      let target = -1;
      tr.doc.forEach((n, pos) => {
        if (target < 0 && at && n.attrs.id === at) target = pos + 1 + Math.min(offset, n.content.size);
      });
      if (target >= 0) tr.setSelection(TextSelection.near(tr.doc.resolve(target)));
    }
    if (tr.docChanged) {
      tr.setMeta('fromStore', true);
      this.view.dispatch(tr);
    } else {
      this.view.dispatch(this.view.state.tr.setMeta('fromStore', true));
    }
  }

  /** Format every live value again (on any change, signals included). */
  async refreshValues(): Promise<void> {
    if (this.destroyed || !this.stored.some((b) => b.marks.some((m) => m.type === 'value'))) return;
    const r = await this.host.values(this.source).catch(() => null);
    if (!r?.ok) return;
    this.values = new Map(Object.entries(r.values ?? {}));
    for (const [k, e] of Object.entries(r.errors ?? {})) this.values.set(k, `⚠ ${e}`);
    for (const v of this.valueViews) v.render();
  }

  valueText(expr: string): string {
    return this.values.get(expr) ?? '…';
  }

  // ---- editor → store ------------------------------------------------

  private touched() {
    this.dirty = true;
    this.status();
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.flush(), PAUSE_MS);
  }

  /** Send what is unsent now (a pause, Cmd+Z, leaving the panel). */
  async flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    while (this.inflight) await this.inflight;
    if (!this.dirty || this.destroyed) return;
    this.dirty = false;
    const editorBlocks: { id: string | null; content: ReturnType<typeof blockOf> }[] = [];
    this.view.state.doc.forEach((n) => editorBlocks.push({ id: n.attrs.id, content: blockOf(n) }));
    const { edits } = diff(this.stored, editorBlocks);
    if (!edits.length) {
      this.status();
      return;
    }
    this.inflight = (async () => {
      let unsent = false;
      const r = await this.host.edit(this.source, edits, 'edit narrative').catch((e) => {
        // Not sent (no session): keep the edits for the next pause. A refusal
        // from the store instead shows the store's state again (the read below).
        unsent = true;
        return { ok: false as const, error: String(e), minted: undefined };
      });
      if (unsent) this.dirty = true;
      if (r.ok && r.minted) this.assignIds(r.minted);
      this.error = r.ok ? '' : (r.error ?? 'edit refused');
      if (!r.ok) console.warn('[narrative] edit refused:', r.error, edits);
    })();
    try {
      await this.inflight;
    } finally {
      this.inflight = null;
    }
    this.wantRefresh = false;
    await this.refresh();
  }

  private assignIds(minted: Record<string, string>) {
    const tr = this.view.state.tr;
    this.view.state.doc.forEach((n, pos) => {
      const id = n.attrs.id as string | null;
      if (id && isTemp(id) && minted[id]) tr.setNodeMarkup(pos, undefined, { ...n.attrs, id: minted[id] });
    });
    if (tr.docChanged) this.view.dispatch(tr.setMeta('fromStore', true));
  }

  private async historyStep(which: 'undo' | 'redo') {
    await this.flush();
    if (which === 'undo') this.host.undo();
    else this.host.redo();
  }

  private status() {
    let n = 0;
    this.view?.state.doc.forEach(() => n++);
    this.onStatus({ pending: this.dirty || !!this.inflight, error: this.error, version: this.version, blocks: n });
  }

  // ---- what the panel drives -----------------------------------------

  private afterUpdate() {
    const s = this.view.state;
    const $h = s.selection.$head;
    // The mention menu: `#` and a word right before the cursor.
    let q: MentionQuery | null = null;
    if (s.selection.empty && $h.parent.isTextblock && $h.parent.type !== schema.nodes.code_block) {
      const before = $h.parent.textBetween(0, $h.parentOffset, undefined, '￼');
      const m = /(?:^|\s)#([\p{L}\p{N}_ -]{0,30})$/u.exec(before);
      if (m && !/\s$/.test(m[1]) && !m[1].includes('  ')) {
        const coords = this.view.coordsAtPos($h.pos);
        q = { from: $h.pos - m[1].length - 1, to: $h.pos, query: m[1], left: coords.left, bottom: coords.bottom };
      }
    }
    this.onMention(q);
    const index = $h.depth > 0 ? $h.index(0) : 0;
    let heading = '';
    s.doc.forEach((n, _p, i) => {
      if (i <= index && n.type === schema.nodes.heading) heading = n.textContent;
    });
    this.onCursor({
      block: $h.depth > 0 ? (s.doc.child(index).attrs.id ?? null) : null,
      index,
      heading,
      selection: s.selection.empty ? '' : s.doc.textBetween(s.selection.from, s.selection.to, '\n'),
    });
  }

  /** Replace the `#query` with a mention of `target` reading `label`. */
  insertMention(q: MentionQuery, target: string, label: string) {
    const mark = schema.marks.mention.create({ target });
    const tr = this.view.state.tr.replaceWith(q.from, q.to, [schema.text(label, [mark]), schema.text(' ')]);
    tr.setSelection(TextSelection.create(tr.doc, q.from + label.length + 1));
    this.labels.set(target, { label, stale: false });
    this.view.dispatch(tr);
    this.view.focus();
  }

  /** A live value of `leaf` (`#4.size.w`) at the cursor. */
  insertValue(target: string, unit: string | null, digits: number | null) {
    const node = schema.nodes.value.create({ target, unit, digits });
    this.view.dispatch(this.view.state.tr.replaceSelectionWith(node, false));
    this.view.focus();
    void this.flush().then(() => this.refreshValues());
  }

  /** Make the cursor's block `kind` (the toolbar). */
  setKind(kind: string, attrs: Record<string, unknown> = {}) {
    const type = schema.nodes[nodeName(kind)];
    if (!type) return;
    const cmd = setBlockType(type, attrs);
    cmd(this.view.state, this.view.dispatch);
    this.view.focus();
  }

  /** A view block after the cursor's block, showing `show`. */
  insertViewBlock(show: string, caption: string) {
    const s = this.view.state;
    const $h = s.selection.$head;
    const end = $h.depth > 0 ? $h.after(1) : s.doc.content.size;
    const node = schema.nodes.view.create({ show, focus: '' }, caption ? schema.text(caption) : undefined);
    this.view.dispatch(s.tr.insert(end, node));
    this.view.focus();
  }

  toggle(mark: 'strong' | 'em' | 'code') {
    toggleMark(schema.marks[mark])(this.view.state, this.view.dispatch);
    this.view.focus();
  }

  /** Scroll a block into view and put the cursor at its end. */
  reveal(id: string) {
    let pos = -1;
    this.view.state.doc.forEach((n, p) => {
      if (n.attrs.id === id) pos = p + n.nodeSize - 1;
    });
    if (pos < 0) return;
    this.view.dispatch(this.view.state.tr.setSelection(TextSelection.near(this.view.state.doc.resolve(pos))).scrollIntoView().setMeta('fromStore', true));
  }

  /** The view blocks' DOM, for the panel's scroll-to-top observer. */
  viewBlocks(): HTMLElement[] {
    return [...this.view.dom.querySelectorAll<HTMLElement>('.nb-view')];
  }

  /** The stored blocks as last read (for reviews of a model turn). */
  storedBlocks(): StoredBlock[] {
    return this.stored.map((b) => ({ ...b, marks: b.marks.map((m) => ({ ...m })) }));
  }

  /** Mark blocks in the editor (a model turn's changes) until cleared. */
  highlight(ids: Set<string>) {
    this.changed = new Set(ids);
    this.view.dispatch(this.view.state.tr.setMeta('fromStore', true));
  }

  registerValue(v: ValueView) {
    this.valueViews.add(v);
  }
  unregisterValue(v: ValueView) {
    this.valueViews.delete(v);
  }
  showView(show: string, focus: string) {
    this.shown = show;
    this.host.showView(show, focus);
    this.view.dispatch(this.view.state.tr.setMeta('fromStore', true));
  }
}

/** A live value: an atom whose text is the formatted value. */
class ValueView implements NodeView {
  dom: HTMLElement;
  constructor(private node: PMNode, private editor: NarrativeEditor) {
    this.dom = document.createElement('span');
    this.dom.className = 'nb-value';
    this.dom.title = node.attrs.target;
    this.dom.dataset.target = node.attrs.target;
    this.render();
    editor.registerValue(this);
  }
  render() {
    this.dom.textContent = this.editor.valueText(valueExpr(this.node.attrs as { target: string; unit: string | null; digits: number | null }));
  }
  update(node: PMNode) {
    if (node.type !== this.node.type) return false;
    this.node = node;
    this.dom.title = node.attrs.target;
    this.render();
    return true;
  }
  destroy() {
    this.editor.unregisterValue(this);
  }
  ignoreMutation() {
    return true;
  }
}

/** A view block: a header that shows its composition, and an editable caption. */
class ViewBlockView implements NodeView {
  dom: HTMLElement;
  contentDOM: HTMLElement;
  private header: HTMLButtonElement;
  constructor(private node: PMNode, _view: EditorView, _getPos: () => number, private editor: NarrativeEditor) {
    this.dom = document.createElement('div');
    this.dom.className = 'nb-view';
    this.header = document.createElement('button');
    this.header.className = 'nb-view-header';
    this.header.contentEditable = 'false';
    this.header.type = 'button';
    this.header.addEventListener('mousedown', (e) => e.preventDefault());
    this.header.addEventListener('click', () => this.editor.showView(this.node.attrs.show, this.node.attrs.focus));
    this.contentDOM = document.createElement('div');
    this.contentDOM.className = 'nb-view-caption';
    this.dom.append(this.header, this.contentDOM);
    this.render();
  }
  private render() {
    const { show, focus } = this.node.attrs;
    this.dom.dataset.show = show;
    this.dom.dataset.focus = focus;
    this.header.textContent = `▸ Show ${show || '(no composition)'}${focus ? ` at ${focus}` : ''}`;
  }
  update(node: PMNode) {
    if (node.type !== this.node.type) return false;
    this.node = node;
    this.render();
    return true;
  }
  ignoreMutation(m: MutationRecord | { type: 'selection'; target: Node }) {
    return m.type !== 'selection' && !this.contentDOM.contains(m.target);
  }
}

/** Every top-level block has a unique id: new ones get a temporary one. */
function tempIds(next: () => string) {
  return new Plugin({
    appendTransaction(trs, _old, state) {
      if (!trs.some((t) => t.docChanged)) return null;
      const seen = new Set<string>();
      const tr = state.tr;
      state.doc.forEach((n, pos) => {
        const id = n.attrs.id as string | null;
        if (!id || seen.has(id)) tr.setNodeMarkup(pos, undefined, { ...n.attrs, id: next() });
        else seen.add(id);
      });
      if (!tr.docChanged) return null;
      const fromStore = trs.every((t) => t.getMeta('fromStore'));
      return fromStore ? tr.setMeta('fromStore', true) : tr;
    },
  });
}

/** Markdown-like shortcuts at the start of a block. */
function rules(): InputRule[] {
  const t = (re: RegExp, type: NodeType, attrs?: (m: RegExpMatchArray) => Record<string, unknown>) => textblockTypeInputRule(re, type, attrs);
  return [
    t(/^(#{1,6})\s$/, schema.nodes.heading, (m) => ({ level: m[1].length })),
    t(/^\s*([-+*])\s$/, schema.nodes.bullet),
    t(/^(\d+)\.\s$/, schema.nodes.ordered),
    t(/^>\s$/, schema.nodes.quote),
    t(/^```$/, schema.nodes.code_block),
  ];
}

/** Backspace at the start of a heading, item or quote makes it a paragraph. */
const toParagraphAtStart: Command = (state, dispatch) => {
  const { $head, empty } = state.selection;
  if (!empty || $head.parentOffset > 0 || $head.parent.type === schema.nodes.paragraph || $head.depth !== 1) return false;
  return setBlockType(schema.nodes.paragraph)(state, dispatch);
};

/** Stale mentions struck through; a mention reading its target's name as a chip. */
function mentionDecorations(doc: PMNode, labels: Map<string, { label: string; stale: boolean }>, shown: string, changed: Set<string>): DecorationSet {
  const decos: Decoration[] = [];
  doc.forEach((node, pos) => {
    if (changed.has(node.attrs.id)) decos.push(Decoration.node(pos, pos + node.nodeSize, { class: 'nb-changed' }));
  });
  doc.descendants((node, pos) => {
    if (node.type === schema.nodes.view && shown && node.attrs.show === shown) decos.push(Decoration.node(pos, pos + node.nodeSize, { class: 'nb-view-shown' }));
    if (!node.isText) return;
    const m = node.marks.find((x) => x.type === schema.marks.mention);
    if (!m) return;
    const info = labels.get(m.attrs.target);
    const cls = [info?.stale ? 'nb-stale' : '', info && node.text === info.label ? 'nb-chip' : ''].filter(Boolean).join(' ');
    if (cls) decos.push(Decoration.inline(pos, pos + node.nodeSize, { class: cls, title: info?.stale ? `${m.attrs.target} no longer exists` : `${info?.label ?? ''} (${m.attrs.target})` }));
  });
  return DecorationSet.create(doc, decos);
}
