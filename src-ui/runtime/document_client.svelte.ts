/**
 * The shell's handle on the authored document: save, load, and views.
 *
 * Everything crosses as messages keyed by `requestId`, so this works the same
 * over the worker's postMessage and the main-thread adapter. The store pushes
 * `documentChanged` whenever its version advances, and the client re-reads
 * the view it is showing. Reactive fields are Svelte 5 runes, so components
 * bind to them directly.
 *
 * Also exposed as `window.__iron` for driving and debugging:
 *   await __iron.save()            -> canonical JSON string
 *   await __iron.load(text)        -> replaces the document
 *   await __iron.view({fidelity:'summary', depth: 2})
 *   await __iron.edit('<edit>…</edit>')   -> apply markup as the model would
 *   await __iron.ask('make the circle red') -> a real model turn
 *   await __iron.bench(20, 200)    -> the solver benchmark in this build (JSON)
 *   __iron.selection()             -> picked node ids, in pick order
 *   await __iron.link('#a', '#b', keep?) -> link two pictures (ECM §7.2)
 *   await __iron.counterparts('#a')     -> every picture of #a's entity
 *   await __iron.focus('#a')       -> "Show in": frame #a in its panels
 *   await __iron.session()         -> the session's selection [{node, panel, chain, …}]
 *   await __iron.stress(5000)      -> add the render benchmark's `stress` composition
 *   await __iron.meters(100)       -> add the instance-cache benchmark's `meters` composition
 *   await __iron.timings(reset?)   -> per-phase CPU render timings since the last reset, raster tiles under `raster`
 *   await __iron.raster(enabled?, budgetMb?) -> raster tiles on (default) or off, every view live, and the atlas budget (0: 256 MB); returns the raster report
 *   await __iron.signals()         -> runtime signals: producers, published leaves, conflicts
 *   await __iron.producer('gas', 'freeze' | 'stop' | 'run') -> commit a producer's values as one transaction and stop it, stop, or run
 *   __iron.copied()                -> the last fragment Cmd+C or Cmd+X copied in this tab
 *   __iron.paste(text?)            -> paste a fragment (default: the last copied), as a paste event does
 *   await __iron.exportSvg(scope?) -> SVG text of a composition (id or name), a node, or 'selection' (default)
 *   await __iron.importSvg(text)   -> import SVG as a paste: {ok, cuts, nodes, anchors, definitions, references}; placed next frame
 *   await __iron.importSvg(text, {share: false}) -> the same with repeated pieces drawn out, no references
 *   await __iron.computed('#a')    -> #a's resolved behaviour, each value with its rule (contexts.md §2.6): {values, text}
 *   await __iron.contexts()        -> the built-in contexts (`std:*`) in their markup
 *   await __iron.viewContext('viewer') -> a view's focus (world, scope, states), palette and keys, each with its rule
 *   await __iron.netlist('circuit') -> a schematic sheet's netlist, SPICE-like, with each net's voltage
 *   __iron.scope()                 -> the open scope (plans/specs/scopes.md): {current, readOnly, notice, storage}
 *   await __iron.flush()           -> write the open scope's pending transactions now
 *   await __iron.scopes.list()     -> stored scopes with sizes; also create(kind?, name?), open(id), rename(name),
 *                                     duplicate(), remove(id), exportText(), importText(text, name), takeOver()
 *   __iron.undo()                  -> undo, as Cmd+Z does
 *
 * Scopes persist to OPFS unless the page is ephemeral: `?demo` (the driver
 * adds it), `?stress=` or `?meters=`, the native build, or no OPFS. Then the
 * demo opens unsaved, as before scopes.
 */
import { QuerySubscriptions } from './query_subscriptions';
import { PendingRequests } from './pending_requests';
import type { ClientRequest, RequestPayload, UiRequest, UiEvent } from './protocol';
import { lastCopied } from './clipboard';
import { documentArguments, type DocumentCommand } from './protocol.generated';
import { Scopes, webLocks, type ScopeState } from './scopes';
import { OpfsScopeStorage } from './scope_storage';
import { persistent, scopesUrl, showScope, target } from '../route';
import { isNativeHost } from './native_host';
import { ANTHROPIC_API_MODELS, isLoopbackOrigin, localEndpoint, type AgentProvider, type ModelEntry } from '../model/runtime-options';
import { listLocalModels } from '../model/transport';
import { MODEL_ASK_TIMEOUT_MS, STOPPED, type AgentEvent, type AskLive } from '../model/progress';
import { discoverLocalAgent } from '../model/local-pairing';
import { discoverNativeAgent } from '../model/native-pairing';
import type { ConversationOptions } from '../model/conversation';

/** Whether this page keeps its document (scopes.md D11). */
export type Fidelity = 'skeleton' | 'summary' | 'full';

export interface ViewQuery {
  scope?: string | null;
  fidelity: Fidelity;
  depth?: number | null;
  /** Instances whose derived insides to show (component-model.md §9). */
  expand?: string[];
}

/** The pointer's tools and their keys (Figma's). */
export type Tool = 'select' | 'pen' | 'rect' | 'ellipse' | 'text' | 'port' | 'empty' | 'table' | 'wire' | `place:${string}`;

/** A composition root of the document, or of the session (the timeline,
 *  Stats, the library's), as `compositions` lists them. */
export interface CompositionEntry { id: string; name: string | null; dims: string; session: boolean }

/** What the dock shows views of: a stored scope (its id and saved layout),
 *  or an unsaved page (`page`, its layout in browser storage). */
export interface OpenedDocument { key: string; saved: string | null }

const PAGE_LAYOUT_KEY = 'iron.workspace.v9';
/** The agent runtime and the model picked for each (the composer's pickers). */
const AGENT_CHOICE_KEY = 'iron.agent.v1';

export class DocumentClient {
  view = $state('');
  version = $state(0);
  fidelity: Fidelity = $state('full');
  depth: number | null = $state(null);
  error = $state('');

  // The model surface's state, for the prompt panel.
  apiKey = $state('');
  modelName = $state('claude-opus-5');
  agentProvider: AgentProvider = $state('codex');
  agentEndpoint = $state('http://127.0.0.1:4318');
  agentToken = $state('');
  agentModel = $state('');
  /** What the chosen runtime offers, for the model picker; `agentModelsFor`
   *  says whose list it is (empty until the bridge answers). */
  agentModels: ModelEntry[] = $state([]);
  agentModelsFor: AgentProvider | null = $state(null);
  /** The model picked for each runtime, remembered in this browser. */
  private modelChoices: Partial<Record<AgentProvider, string>> = {};
  agentPairing = $state(false);
  private pairedToken = $state('');
  private pairedEndpoint = $state('');
  private pairingSequence = 0;
  private nativeAgentPaired = false;
  agentAutoPaired = $derived(!!this.agentToken && this.agentToken === this.pairedToken && this.agentEndpoint === this.pairedEndpoint);
  canAsk = $derived(this.agentProvider === 'anthropic' ? !!this.apiKey : !!this.agentToken);
  asking = $state(false);
  lastAsk: any = $state(null);

  // What the pointer picked and hovers, as node ids (pushed by the engine).
  selection: string[] = $state([]);
  hovered: string[] = $state([]);
  /** Every picture of the first selected node's entity, for "Show in". */
  counterparts: { entity: string | null; figures: { node: string; composition: string | null; name: string | null }[] } =
    $state({ entity: null, figures: [] });
  /** The tool the pointer drives (`select`, `pen`, `rect`, `ellipse`), as
   *  the engine reports it. */
  tool: Tool = $state('select');
  /** What each view offers, by panel, resolved from its focus context
   *  (plans/specs/contexts.md §2.7): its palette (`select rect|ellipse pen`,
   *  `|` sharing a slot) and keymap (`v:select r:rect`). */
  focus: Record<string, { palette: string; keys: string }> = $state({});
  /** Font families whose files have arrived (plans/specs/text.md §4), so
   *  the picker can mark what will draw at once. */
  loadedFonts: string[] = $state([]);
  /** The canvas text edit, as the engine last reported it: which node, and
   *  the selection (UTF-16 offsets). Styling goes to a selected range. */
  textEdit: { active: boolean; node: string; anchor: number; focus: number } = $state({ active: false, node: '', anchor: 0, focus: 0 });
  /** Editing a shape's points would cut bindings or clips: what they are,
   *  for the dialog. Answered with `toPath(node, true)` or dismissed. */
  convertQuestion: { node: string; type: string; cuts: string[] } | null = $state(null);
  /** A link dropped on a main's part, waiting for its mapping
   *  (component-authoring.md §3; `lib/LinkPopover.svelte`). */
  linkDrop: { spec: string; node: string; x: number; y: number } | null = $state(null);
  /** A pending merge question from Link (ECM I5): the person picks the survivor. */
  question: any = $state(null);

  private post: ((data: UiRequest) => void) | null = null;
  private pending = new PendingRequests();
  /** Progress listeners of running model turns, by request id. */
  private askListeners = new Map<number, (event: AgentEvent) => void>();
  private subscriptions = new QuerySubscriptions(command => this.request('documentCall', documentArguments(command)), interest => this.post?.({ty:'setUiInterest', ...interest}));
  sessionVersion = $state(0);
  resolvedVersion = $state(0);
  canUndo = $state(false);
  canRedo = $state(false);
  generation = $state(0);
  documentEpoch = $state(0);
  private sessionId = 0;
  private viewGeneration = 0;
  private counterpartGeneration = 0;
  private stopCounterparts: (() => void) | null = null;
  /** A document to load into the next engine: what a restart (the
   *  Settings view) carries over on an unsaved page. */
  private carried: string | null = null;
  /** Whether the page has made its first document (the boot: a scope, or
   *  the demo with `?stress=`, `?meters=` and the showcase). */
  private booted = false;
  /** Whether this session's engine has been given its document. */
  private sessionStarted = false;
  /** Every composition root, kept current while the engine runs (the
   *  dock's tab titles and its missing-composition views read it).
   *  `compositionsKnown` turns true at the first answer for this document. */
  compositions: CompositionEntry[] = $state([]);
  compositionsKnown = $state(false);
  private compositionWatchers = new Set<(list: CompositionEntry[]) => void>();
  private stopCompositions: (() => void) | null = null;
  /** The document the dock last laid out, and who wants to know. */
  private openedDocument: OpenedDocument | null = null;
  private openedWatchers = new Set<(opened: OpenedDocument) => void>();
  /** The open scope, kept durable; null on an ephemeral page. One per page,
   *  across engine restarts. */
  readonly scopes: Scopes | null = persistent()
    ? new Scopes({ call: (fn, args) => this.call(fn, args), save: () => this.save() }, new OpfsScopeStorage(), webLocks())
    : null;
  scope: ScopeState = $state({ current: null, readOnly: false, notice: '', storage: 'off' });
  private pageHooks = false;

  init(post: (data: UiRequest) => void, sessionId: number) {
    this.dispose();
    this.sessionId = sessionId;
    this.generation++;
    this.pending.attach(sessionId);
    this.post = post;
    this.sessionStarted = false;
    this.scopes?.pause();
    if (this.scopes && !this.pageHooks) {
      this.pageHooks = true;
      // The address names the open scope, so a reload reopens it.
      this.scopes.onChange = state => { this.scope = state; if (state.current) showScope(state.current.id); };
      this.scopes.onOpened = (meta, workspace) => void this.documentOpened({ key: meta.id, saved: workspace });
      // Write the last batch when the tab is hidden or closed (§4.2).
      document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') void this.scopes!.flush(); });
      window.addEventListener('pagehide', () => void this.scopes!.flush());
    }
    this.error = '';
    try {
      this.apiKey = localStorage.getItem('iron.anthropicKey') ?? '';
      const saved = JSON.parse(localStorage.getItem(AGENT_CHOICE_KEY) ?? 'null');
      if (saved && ['codex', 'claude', 'anthropic'].includes(saved.provider)) {
        this.modelChoices = saved.models ?? {};
        this.agentProvider = saved.provider;
        this.agentModel = saved.provider === 'anthropic' ? '' : this.modelChoices[saved.provider as AgentProvider] ?? '';
        if (this.modelChoices.anthropic) this.modelName = this.modelChoices.anthropic;
      }
    } catch {
      // storage unavailable; the key is entered per session
    }
    void this.loadAgentModels();
    void this.connectLocalAgent();
    (window as any).__iron = {
      // Diagnostic access to JSON-returning operations, including contract errors.
      call: (name: string, args: unknown[] = []) => this.call(name, args),
      subscriptions: () => this.subscriptions.stats(),
      protocolState: () => ({ generation: this.generation, pending: this.pending.size, session: this.sessionId, version: this.version, sessionVersion: this.sessionVersion, resolvedVersion: this.resolvedVersion, canUndo: this.canUndo, canRedo: this.canRedo }),
      save: () => this.save(),
      load: (text: string) => this.load(text),
      scope: () => this.scopes?.state() ?? this.scope,
      flush: () => this.scopes?.flush(),
      // The scope menu's actions (scopes.md §6), for the driver.
      scopes: {
        list: () => this.scopes?.list() ?? Promise.resolve([]),
        create: (kind: 'empty' | 'demo' = 'empty', name?: string) => this.scopes?.create(kind, name),
        open: (id: string) => this.scopes?.open(id),
        rename: (name: string) => this.scopes?.rename(name),
        duplicate: () => this.scopes?.duplicate(),
        remove: (id: string) => this.scopes?.remove(id),
        exportText: () => this.scopes?.exportText(),
        importText: (text: string, name: string) => this.scopes?.importText(text, name),
        takeOver: () => this.scopes?.takeOver(),
      },
      undo: () => this.undo(),
      view: (q: ViewQuery) => this.request('documentView', { query: { scope: null, depth: null, ...q } }),
      edit: (xml: string, label?: string) => this.edit(xml, label),
      ask: (instruction: string) => this.ask(instruction),
      configureAgent: (options: { provider: AgentProvider; endpoint?: string; token?: string; model?: string }) => this.configureAgent(options),
      agentState: () => ({ provider: this.agentProvider, endpoint: this.agentEndpoint, paired: this.agentAutoPaired, connecting: this.agentPairing, hasToken: !!this.agentToken, canAsk: this.canAsk }),
      bench: async (iterations = 20, repeats = 200) =>
        JSON.parse(await this.request('solveBench', { iterations, repeats })),
      selection: () => [...this.selection],
      hovered: () => [...this.hovered],
      link: (a: string, b: string, keep = '') => this.link(a, b, keep),
      counterparts: (node: string) => this.command({ op: 'counterparts', node: node }),
      focus: (node: string) => this.command({ op: 'focus', node: node }),
      toPath: (node: string, force = false) => this.toPath(node, force),
      convertQuestion: () => this.convertQuestion,
      // The selection as the store's session records it, with how each
      // node was reached (panel, chain).
      session: () => this.command({ op: 'selection' }),
      // The render benchmark (roadmap stage H): a composition of `shapes`
      // shapes, and per-phase CPU timings (`src/perf.rs`).
      stress: (shapes = 5000) => this.command({ op: 'stress', shapes: shapes }),
      // The instance-cache benchmark: `n` dial meters, the first driven by
      // a slider (plans/specs/instance-cache.md phase 0).
      meters: (n = 100) => this.command({ op: 'meters', n: n }),
      spatial: (enabled = true) => this.command({ op: 'spatial', enabled: enabled }),
      // Object snapping for the select tool (plans/specs/snapping.md).
      snapping: (enabled = true) => this.command({ op: 'snapping', enabled: enabled }),
      // Tables (plans/specs/layout-and-tables.md): make one from a spec
      // ({parent, x, y, rows, preset, header, label_col, emphasis, align}),
      // and edit one's structure ('add_row', 'merge', 'preset:boxed', …).
      table: (spec: object) => this.command({ op: 'table', spec: JSON.stringify(spec) }),
      tableEdit: (table: string, verb: string, cells: string[] = []) => this.command({ op: 'table_edit', table: table, verb: verb, cells: JSON.stringify(cells) }),
      // Contexts (plans/specs/contexts.md): a node's resolved behaviour with
      // the rule behind each value, and the built-in library's markup.
      computed: (node: string) => this.command({ op: 'computed', node: node }),
      contexts: () => this.command({ op: 'contexts' }),
      viewContext: (panel: string) => this.command({ op: 'view_context', panel: panel }),
      // Schematics (plans/specs/schematic.md): a sheet's netlist.
      netlist: (sheet: string) => this.command({ op: 'netlist', sheet: sheet }),
      // The contexts showcase's tabs (added at start unless `?showcase=0`).
      showcase: () => this.command({ op: 'showcase' }),
      timings: (reset = false) => this.command({ op: 'timings', reset: reset }),
      // Raster tiles for cold chunks (render-cache.md §5); off draws live.
      raster: (enabled = true, budgetMb = 0) => this.command({ op: 'raster', enabled: enabled, budget_mb: budgetMb }),
      // The runtime signal layer (vision 10 §4) and its host producers.
      signals: () => this.command({ op: 'signals' }),
      producer: (name = 'gas', action = 'freeze') => this.command({ op: 'producer', name: name, action: action }),
      // Free running (free-running.md): a composition's run mode, by id or
      // name ('auto' | 'always' | 'on_demand'), and whether it is live.
      runMode: (source: string, mode: string) => this.command({ op: 'set_run_mode', source: source, mode: mode }),
      compositionInfo: (source: string) => this.command({ op: 'composition_info', source: source }),
      exportSvg: async (scope = 'selection') => {
        const r = await this.exportSvg(scope);
        if (!r.ok) throw new Error(r.error);
        return r.svg;
      },
      exportSvgCuts: async (scope = 'selection') => (await this.exportSvg(scope)).cuts,
      importSvg: (text: string, opts?: { share?: boolean }) => this.importSvg(text, opts),
      copied: () => lastCopied(),
      paste: (text?: string) => {
        const dt = new DataTransfer();
        dt.setData('text/plain', text ?? lastCopied());
        window.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt }));
      },
    };
  }

  subscribe<T>(command: DocumentCommand, value: (result: T) => void, error?: (error: unknown) => void) {
    return this.subscriptions.subscribe(command, value, error);
  }
  undo() { this.post?.({ ty: 'undo' }); }
  redo() { this.post?.({ ty: 'redo' }); }

  setApiKey(key: string) {
    this.apiKey = key;
    try {
      localStorage.setItem('iron.anthropicKey', key);
    } catch {
      // storage unavailable
    }
  }

  configureAgent(options: { provider: AgentProvider; endpoint?: string; token?: string; model?: string }) {
    if (!['codex', 'claude', 'anthropic'].includes(options.provider)) throw new Error('Unknown agent provider');
    if (options.endpoint !== undefined) localEndpoint(options.endpoint);
    this.agentProvider = options.provider;
    if (options.endpoint !== undefined) this.agentEndpoint = options.endpoint;
    if (options.token !== undefined) this.agentToken = options.token;
    this.agentModel = options.model ?? (options.provider === 'anthropic' ? '' : this.modelChoices[options.provider] ?? '');
    this.saveAgentChoice();
    if (options.provider !== 'anthropic' && options.token === undefined) void this.connectLocalAgent().then(() => this.loadAgentModels());
    else void this.loadAgentModels();
  }

  /** The model for the chosen runtime (`''`: the runtime's default). */
  setAgentModel(model: string) {
    if (this.agentProvider === 'anthropic') this.modelName = model || ANTHROPIC_API_MODELS[0].id;
    else this.agentModel = model;
    this.modelChoices = { ...this.modelChoices, [this.agentProvider]: this.agentProvider === 'anthropic' ? this.modelName : model };
    this.saveAgentChoice();
  }

  private saveAgentChoice() {
    try {
      localStorage.setItem(AGENT_CHOICE_KEY, JSON.stringify({ provider: this.agentProvider, models: this.modelChoices }));
    } catch {
      // storage unavailable: the choice lasts this page
    }
  }

  /** Ask what the chosen runtime offers: the bridge lists Codex's models
   *  for this login and Claude Code's aliases; the API path has its own. */
  async loadAgentModels(): Promise<void> {
    const provider = this.agentProvider;
    if (provider === 'anthropic') {
      this.agentModels = ANTHROPIC_API_MODELS;
      this.agentModelsFor = provider;
      return;
    }
    if (!this.agentToken) return;
    try {
      const models = await listLocalModels(this.agentEndpoint, this.agentToken, provider);
      if (provider === this.agentProvider) { this.agentModels = models; this.agentModelsFor = provider; }
    } catch (e) {
      console.warn(`[agent] could not list ${provider} models`, e);
    }
  }

  async connectLocalAgent(): Promise<boolean> {
    const native = isNativeHost();
    if (!native && (!import.meta.env.DEV || !isLoopbackOrigin(location.origin))) return false;
    if (this.agentProvider === 'anthropic' && (!native || this.nativeAgentPaired)) return false;
    const sequence = ++this.pairingSequence;
    const generation = this.generation;
    const endpoint = this.agentEndpoint;
    const previousToken = this.agentToken;
    const provider = this.agentProvider;
    this.agentPairing = true;
    try {
      const connection = native ? await discoverNativeAgent(command => (window as any).__TAURI__.core.invoke(command)) : null;
      const token = native ? (connection?.endpoint === endpoint ? connection.token : null) : await discoverLocalAgent(endpoint);
      if (!token || sequence !== this.pairingSequence || generation !== this.generation || endpoint !== this.agentEndpoint || previousToken !== this.agentToken || provider !== this.agentProvider) return false;
      if (connection && !this.nativeAgentPaired) {
        this.agentProvider = connection.provider;
        this.agentModel = this.modelChoices[connection.provider] ?? '';
        this.nativeAgentPaired = true;
      }
      this.agentToken = token;
      this.pairedToken = token;
      this.pairedEndpoint = endpoint;
      void this.loadAgentModels();
      return true;
    } finally { if (sequence === this.pairingSequence) this.agentPairing = false; }
  }

  /** Pick a tool, as its key would (V select, P pen, R rectangle, O ellipse). */
  setTool(tool: Tool) {
    this.post?.({ ty: 'toolKey', key: `tool:${tool}`, m: 0 });
  }

  /** Turn a rect or an ellipse into a path, as editing its points does.
   *  Without `force` it refuses when that would cut a binding or a clip,
   *  and lists them in `cuts`. */
  async toPath(node: string, force = false): Promise<{ ok: boolean; cuts?: string[]; error?: string }> {
    return this.command({ op: 'to_path', node: node, force: force });
  }

  /** SVG of a composition (id or name), a node, or `selection`, and what
   *  it could not carry (plans/specs/svg-import-export.md). */
  async exportSvg(scope: string): Promise<{ ok: boolean; svg?: string; cuts?: string[]; error?: string }> {
    const r = await this.command({ op: 'export_svg', scope: scope });
    for (const cut of r.cuts ?? []) console.info(`export SVG: ${cut}`);
    return r;
  }

  /** Import SVG text as a paste: under the pointer or at the view's centre,
   *  one "import SVG" transaction. `cuts` lists what it could not carry. */
  async importSvg(text: string, opts: { share?: boolean } = {}): Promise<{ ok: boolean; cuts?: string[]; nodes?: number; anchors?: number; definitions?: number; references?: number; error?: string }> {
    // Repeated pieces come in as references to definitions; `share: false`
    // draws every one out in full, the reference the sharing is checked
    // against (plans/specs/instance-cache.md phase D).
    const r = await this.command({ op: opts.share === false ? 'import_svg_flat' : 'import_svg', text });
    if (!r.ok) this.error = `import SVG: ${r.error}`;
    return r;
  }

  /** Export `scope` as an .svg download named after it. */
  async downloadSvg(scope: string, name = 'iron'): Promise<void> {
    const r = await this.exportSvg(scope);
    if (!r.ok || !r.svg) {
      this.error = `export SVG: ${r.error ?? 'nothing to export'}`;
      return;
    }
    this.error = '';
    const url = URL.createObjectURL(new Blob([r.svg], { type: 'image/svg+xml' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = `${name.replace(/[^\w.-]+/g, '-')}.svg`;
    a.click();
    URL.revokeObjectURL(url);
  }

  /** Apply `<edit>` markup as the model would. Resolves with the outcome. */
  edit(xml: string, label?: string): Promise<any> {
    return this.request('modelEdit', { xml, label });
  }

  /** One model turn: view, prompt, apply. */
  async ask(instruction: string, conversation: ConversationOptions = {}, live: AskLive = {}): Promise<any> {
    const generation = this.generation;
    if (this.asking) throw new Error('a model turn is already running');
    this.asking = true;
    try {
      if (this.agentProvider !== 'anthropic') await this.connectLocalAgent();
      if (generation !== this.generation) throw new Error('session disposed');
      if (!this.canAsk) throw new Error(this.agentProvider === 'anthropic' ? 'no API key set' : 'start the local agent bridge and connect in Agent settings');
      const runtime = this.agentProvider === 'anthropic' ? undefined : {
        provider: this.agentProvider, endpoint: this.agentEndpoint, token: this.agentToken, model: this.agentModel || undefined,
      };
      if (live.signal?.aborted) throw new Error(STOPPED);
      const payload = { instruction, apiKey: runtime ? '' : this.apiKey, model: this.modelName, runtime, ...conversation };
      let askId = -1;
      const cancel = () => { if (askId >= 0) this.post?.({ ty: 'modelAskCancel', askId } as UiRequest); };
      live.signal?.addEventListener('abort', cancel, { once: true });
      let result;
      try {
        result = await this.pending.request((requestId, sessionId) => {
          askId = requestId;
          if (live.onEvent) this.askListeners.set(requestId, live.onEvent);
          this.post!({ ty: 'modelAsk', requestId, sessionId, ...payload } as UiRequest);
        }, MODEL_ASK_TIMEOUT_MS);
      } finally {
        live.signal?.removeEventListener('abort', cancel);
        this.askListeners.delete(askId);
      }
      if (generation === this.generation) this.lastAsk = result;
      return result;
    } finally {
      if (generation === this.generation) this.asking = false;
    }
  }

  /** Checked, named payloads at the shell boundary; positional FFI stays here. */
  async command<T = any>(command: DocumentCommand): Promise<T> {
    return JSON.parse(await this.request('documentCall', documentArguments(command)));
  }

  /** Diagnostic compatibility entry, intentionally checked by Rust at runtime. */
  async call(fn: string, args: unknown[]): Promise<any> {
    return JSON.parse(await this.request('documentCall', { fn, args }));
  }

  /** Link two pictures (ECM §7.2). A merge question is kept in `question`
   *  for the dialog and returned; answer it by calling again with `keep`. */
  async link(a: string, b: string, keep = ''): Promise<any> {
    const generation = this.generation;
    const r = await this.command({ op: 'link', a: a, b: b, keep: keep, actor: 'human' });
    if (generation === this.generation) this.question = r.question ? { ...r.question, aNode: a, bNode: b } : null;
    return r;
  }

  private refreshCounterparts() {
    this.stopCounterparts?.();
    this.stopCounterparts = null;
    const node = this.selection[0];
    this.counterparts = { entity: null, figures: [] };
    if (node) this.stopCounterparts = this.subscribe<typeof this.counterparts>({ op: 'counterparts', node }, result => { this.counterparts = result; });
  }

  /** Give a new engine its document, which starts empty (no demo precedes
   *  a scope). The first time on a page: the scope the URL names, or on an
   *  unsaved page the demo, the benchmarks' compositions and the showcase.
   *  After a settings restart: the scope reopened from storage, or what
   *  the old engine held (`carryOver`). */
  private async startDocument() {
    const first = !this.booted;
    this.booted = true;
    const q = new URLSearchParams(location.search);
    // The contexts showcase (plans/specs/contexts.md): its outline, pitch
    // and dials compositions. `?showcase=0` leaves it out (the driver does,
    // so its scripts' ids stay those after the demo scene).
    // An engine that already holds a document (the native engine across a
    // webview reload) keeps it: `demo` answers `already`, and so does the
    // showcase, so neither lands on top of it.
    const demo = async () => {
      const r = await this.command<{ ok: boolean; already?: boolean; error?: string }>({ op: 'demo' });
      if (!r.ok) throw new Error(r.error ?? 'the demo was refused');
      if (!r.already && q.get('showcase') !== '0') await this.command({ op: 'showcase' });
      return !r.already;
    };
    if (this.scopes) {
      if (!first) return this.scopes.reopen();
      // Boot (scopes.md D11, §10.2): open the scope the URL names
      // (route.ts); one that no longer exists goes back to the scopes
      // page. `&export` downloads it once open (the scopes page's Export,
      // for a scope whose log needs replaying). Read before the boot
      // rewrites the address to the open scope.
      const exporting = q.has('export');
      const r = await this.scopes.boot(async () => { await demo(); }, target());
      if (r === 'missing') location.replace(scopesUrl());
      else if (exporting) void this.download();
      return;
    }
    if (this.carried !== null) {
      const text = this.carried;
      this.carried = null;
      await this.load(text);
    } else if (await demo()) {
      // The benchmarks (`?stress=N`, `?meters=N`) add their composition to
      // the demo, once per page; their tabs are in the default layout.
      const shapes = Number(q.get('stress'));
      if (first && shapes > 0) await this.command({ op: 'stress', shapes: shapes });
      const meters = Number(q.get('meters'));
      if (first && meters > 0) await this.command({ op: 'meters', n: meters });
    }
    let saved: string | null = null;
    try {
      saved = q.get('layout') === 'default' ? null : localStorage.getItem(PAGE_LAYOUT_KEY);
    } catch {
      // storage unavailable: the default layout
    }
    await this.documentOpened({ key: 'page', saved });
  }

  /** The document is in the engine: refresh the composition list (so no
   *  view briefly reads as missing), then tell the dock. */
  private async documentOpened(opened: OpenedDocument) {
    await this.settled();
    await this.refreshCompositions();
    this.openedDocument = opened;
    for (const watcher of this.openedWatchers) watcher(opened);
  }

  /** Wait until a load the engine is holding for its next frame is in. */
  private async settled() {
    for (let i = 0; i < 600; i++) {
      // Past every version: only `pending` matters, not the log itself.
      const r = await this.call('scope_log', [Number.MAX_SAFE_INTEGER]);
      if (!r.pending) return;
      await new Promise(resolve => setTimeout(resolve, 25));
    }
  }

  /** Call `fn` with each document the dock should lay out: now, if one is
   *  open, and at every later open or scope switch. */
  onDocumentOpened(fn: (opened: OpenedDocument) => void): () => void {
    this.openedWatchers.add(fn);
    if (this.openedDocument) fn(this.openedDocument);
    return () => this.openedWatchers.delete(fn);
  }

  /** Keep the dock's layout with the open document: the scope's
   *  `workspace.json`, or browser storage on an unsaved page. */
  writeWorkspace(key: string, json: string) {
    if (key === 'page') {
      try { localStorage.setItem(PAGE_LAYOUT_KEY, json); } catch { /* this session only */ }
    } else if (this.scopes?.state().current?.id === key) {
      void this.scopes.saveWorkspace(json).catch(e => console.warn('[scopes] layout not saved', e));
    }
  }

  /** Forget the open document's saved layout (Reset layout). */
  forgetWorkspace(key: string) {
    if (key === 'page') {
      try { localStorage.removeItem(PAGE_LAYOUT_KEY); } catch { /* nothing saved */ }
    }
  }

  private watchCompositionList() {
    this.stopCompositions?.();
    this.stopCompositions = this.subscribe<CompositionEntry[]>({ op: 'compositions' }, list => this.setCompositions(list));
  }

  private setCompositions(list: CompositionEntry[]) {
    this.compositions = list;
    this.compositionsKnown = true;
    for (const watcher of this.compositionWatchers) watcher(list);
  }

  /** Read the composition list now, rather than at the next change. */
  async refreshCompositions(): Promise<CompositionEntry[]> {
    const list = await this.command<CompositionEntry[]>({ op: 'compositions' });
    this.setCompositions(list);
    return list;
  }

  /** Call `fn` whenever the composition list changes (a composition made,
   *  renamed or deleted, a scope switched). */
  watchCompositions(fn: (list: CompositionEntry[]) => void): () => void {
    this.compositionWatchers.add(fn);
    return () => this.compositionWatchers.delete(fn);
  }

  /** Returns true if the message was one of ours. */
  /** Bold, italic, underline or strikethrough on the selected characters
   *  of the canvas text edit (the engine decides on or off). */
  textToggle(what: 'bold' | 'italic' | 'underline' | 'strike') {
    this.post?.({ ty: 'textToggle', what });
  }

  fontsLoaded(families: string[]) {
    this.loadedFonts = [...new Set([...this.loadedFonts, ...families])];
  }

  handleMessage(data: UiEvent): boolean {
    if (!this.post || data.sessionId !== this.sessionId) return false;
    switch (data?.ty) {
      case 'selection':
        this.selection = (data.list ?? []).map(String);
        void this.refreshCounterparts();
        return true;
      case 'hover':
        this.hovered = (data.list ?? []).map(String);
        return true;
      case 'enginePrepared':
        this.subscriptions.start();
        this.watchCompositionList();
        if (!this.sessionStarted) {
          this.sessionStarted = true;
          void this.startDocument().catch(e => { console.error('[boot] the document did not open', e); this.error = String(e); });
        }
        return true;
      case 'stateChanged':
        if (data.reset) this.documentEpoch++;
        this.version = data.version;
        this.sessionVersion = data.sessionVersion;
        this.resolvedVersion = data.resolvedVersion;
        this.canUndo = data.canUndo;
        this.canRedo = data.canRedo;
        this.scopes?.changed(data.version);
        this.subscriptions.invalidate(data);
        return true;
      case 'documentChanged':
        this.version = data.version;
        this.scopes?.changed(data.version);
        return true;
      case 'documentSaved':
      case 'documentLoaded':
      case 'documentView':
      case 'documentCall':
      case 'solveBench': {
        this.pending.settle(data.requestId, data.sessionId, 'text' in data ? data.text : undefined, 'error' in data ? data.error : undefined);
        return true;
      }
      case 'modelAskProgress': {
        if (data.sessionId === undefined || data.sessionId === this.pending.session) for (const event of data.events) this.askListeners.get(data.requestId)?.(event);
        return true;
      }
      case 'modelEditResult':
      case 'modelAskResult': {
        // Resolve with the structured outcome even on a rejected edit: the
        // failure markup is the useful part.
        this.pending.settle(data.requestId, data.sessionId, 'outcome' in data ? data.outcome : 'result' in data ? data.result : undefined, 'error' in data ? data.error : undefined);
        return true;
      }
      default:
        return false;
    }
  }

  dispose() {
    this.post = null;
    this.pending.dispose();
    this.stopCounterparts?.();
    this.stopCounterparts = null;
    this.stopCompositions?.();
    this.stopCompositions = null;
    this.compositionsKnown = false;
    this.subscriptions.dispose();
    this.canUndo = false;
    this.canRedo = false;
    this.selection = [];
    this.hovered = [];
    this.counterparts = { entity: null, figures: [] };
    this.question = null;
    this.view = '';
    this.viewGeneration++;
    this.counterpartGeneration++;
    this.generation++;
    this.asking = false;
  }

  private request<T extends ClientRequest['ty']>(ty: T, payload: RequestPayload<T>): Promise<any> {
    if (!this.post) return Promise.reject(new Error('no session'));
    return this.pending.request((requestId, sessionId) => this.post!({ ty, requestId, sessionId, ...payload } as UiRequest), ty === 'modelAsk' ? MODEL_ASK_TIMEOUT_MS : 15000);
  }

  async refresh(): Promise<void> {
    const generation = ++this.viewGeneration;
    try {
      const view = await this.request('documentView', {
        query: { scope: null, fidelity: this.fidelity, depth: this.depth },
      });
      if (generation === this.viewGeneration) { this.view = view; this.error = ''; }
    } catch (e) {
      if (generation === this.viewGeneration) this.error = String(e);
    }
  }

  /** Load `text` into the next app once it is up (`null`: start fresh). */
  carryOver(text: string | null): void {
    this.carried = text;
  }

  save(): Promise<string> {
    return this.request('documentSave', {});
  }

  async load(text: string): Promise<void> {
    const generation = this.generation;
    try {
      await this.request('documentLoad', { text });
      if (generation === this.generation) this.error = '';
    } catch (e) {
      if (generation === this.generation) this.error = String(e);
      throw e;
    }
  }

  /** Save to a download, named after the open scope (Export in the scope
   *  menu), or iron-document.json on an ephemeral page. */
  async download(): Promise<void> {
    const { name, text } = this.scopes ? await this.scopes.exportText() : { name: 'iron-document', text: await this.save() };
    const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = `${name.replace(/[^\w.-]+/g, '-') || 'iron-document'}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  /** Open a saved document, or import an .svg into the view as a paste. */
  async openFile(file: File): Promise<void> {
    const text = await file.text();
    if (file.name.toLowerCase().endsWith('.svg') || file.type === 'image/svg+xml') {
      await this.importSvg(text);
      return;
    }
    // A saved document opens as a new scope (scopes.md D4).
    if (this.scopes) await this.scopes.importText(text, file.name.replace(/\.json$/i, ''));
    else await this.load(text);
  }
}
