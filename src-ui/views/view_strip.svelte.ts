// The view selector strip's data (plans/specs/panel-primitives.md §5): the
// views `std:workspace`'s `workspace.strip` offers, as dock view specs. The
// options are context data (a document overrides them like any context), the
// choice is the dock's active view in the group, and the control is
// ViewStrip.svelte.
import type { Component } from "svelte";
import BookOpen from "@lucide/svelte/icons/book-open";
import FileCode from "@lucide/svelte/icons/file-code";
import Layers from "@lucide/svelte/icons/layers";
import LibraryBig from "@lucide/svelte/icons/library-big";
import MessageSquare from "@lucide/svelte/icons/message-square";
import PanelLeft from "@lucide/svelte/icons/panel-left";
import Shapes from "@lucide/svelte/icons/shapes";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import Clock from "@lucide/svelte/icons/clock";
import { controllerManager } from "../controller-manager.svelte";
import type { Notation, ViewSpec } from "./views";

export type StripItem = { key: string; label: string; icon: Component<any>; spec: ViewSpec; id?: string };

/** The built-ins' short names: their view spec, dock id, label and icon. */
const BUILT_IN: Record<string, Omit<StripItem, "key">> = {
  layers: { label: "Layers", icon: Layers, spec: { source: "follow", notation: "layers" }, id: "layers" },
  model: { label: "Model (the agent)", icon: MessageSquare, spec: { source: "document", notation: "conversation" }, id: "model" },
  library: { label: "Library", icon: LibraryBig, spec: { source: "library", notation: "composition" }, id: "library" },
  inspector: { label: "Inspector", icon: SlidersHorizontal, spec: { source: "selection", notation: "selection" }, id: "selection" },
  timeline: { label: "Timeline", icon: Clock, spec: { source: "timeline", notation: "composition" }, id: "timeline" },
  document: { label: "Document", icon: FileCode, spec: { source: "document", notation: "markup" }, id: "document" },
};
const ICONS: Partial<Record<Notation, Component<any>>> = { composition: Shapes, narrative: BookOpen, layers: Layers, conversation: MessageSquare, selection: SlidersHorizontal, markup: FileCode };

class ViewStrip {
  items: StripItem[] = $state([]);
  #timer: ReturnType<typeof setInterval> | null = null;
  #users = 0;

  async read() {
    try {
      const doc = controllerManager.docClient;
      const r = await doc.command<{ ok: boolean; strip: string[] }>({ op: "workspace_strip" });
      if (!r.ok) return;
      let narratives: { id: string; name: string }[] = [];
      if (r.strip.includes("narratives")) {
        const n = await doc.command<{ ok: boolean; narratives: { id: string; name: string }[] }>({ op: "narrative_list" });
        narratives = n.ok ? n.narratives : [];
      }
      const out: StripItem[] = [];
      for (const word of r.strip) {
        if (word === "narratives") {
          for (const n of narratives) out.push({ key: `narrative:${n.name || n.id}`, label: n.name || "Narrative", icon: BookOpen, spec: { source: n.name || n.id, notation: "narrative" } });
        } else if (BUILT_IN[word]) {
          out.push({ key: word, ...BUILT_IN[word] });
        } else {
          const [notation, source] = [word.slice(0, word.indexOf(":")), word.slice(word.indexOf(":") + 1).replaceAll("_", " ")];
          out.push({ key: word, label: source, icon: ICONS[notation as Notation] ?? PanelLeft, spec: { source, notation: notation as Notation } });
        }
      }
      if (JSON.stringify(out.map((i) => i.key)) !== JSON.stringify(this.items.map((i) => i.key))) this.items = out;
    } catch {
      // No engine session yet.
    }
  }

  use(): () => void {
    if (this.#users++ === 0) {
      void this.read();
      // Contexts and narratives change rarely: a slow poll.
      this.#timer = setInterval(() => void this.read(), 2000);
    }
    return () => {
      if (--this.#users === 0 && this.#timer) {
        clearInterval(this.#timer);
        this.#timer = null;
      }
    };
  }

  /** The strip item a view is, if any. */
  itemOf(spec: ViewSpec): StripItem | undefined {
    return this.items.find((i) => i.spec.notation === spec.notation && i.spec.source === spec.source);
  }
}

export const viewStrip = new ViewStrip();
