//! Each view's focus context (plans/specs/contexts.md §2.7): the world its
//! tools create into, the container its selection is open in, and its states
//! (`editing-text`, `editing-points`, `main`). What the view offers resolves
//! against it, as context data: its palette, which the shell draws, and its
//! keymap, through which plain letter keys reach tools; its command keymap
//! (`keys.commands`, std:workspace's merged under the view's own), through
//! which the shell's chords reach commands; and its right-click menu
//! (`menu.canvas`). No tool, command or key is named here.

use bevy::prelude::*;
use iron_document::composition::{self, Dims};
use iron_document::contexts::{self, Field, Focus, Resolver};
use iron_document::NodeId;

use crate::bevy_app::select::{PathEdit, SelectTool, ToolKeys};
use crate::bevy_app::text_tool::TextTool;
use crate::document_bridge::{DocumentStore, PanelViews, nest};
use crate::host_events::{EngineEvent, HostEvents};

/// What a view offers: its palette and keymaps, as context values.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub palette: String,
    pub keys: String,
    /// Chords to commands, std:workspace's under the view's own.
    pub commands: String,
    /// Its right-click menu: command ids, `|` between groups.
    pub menu: String,
    pub tolerances: crate::bevy_app::tolerances::Tolerances,
    pub chrome: Chrome,
    /// The grid placement, wiring and moves round to, px (`snap.grid`; 0
    /// for none).
    pub grid: f64,
}

/// How a view's chrome looks (`chrome.*`, plans/specs/chrome.md's revision):
/// the overlay drawer reads it. The defaults are today's look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    /// Boxes, grips, outlines and handles, when a context sets a colour;
    /// `None` keeps the built-in blues.
    pub accent: Option<[f32; 4]>,
    /// A box's grip size, CSS px.
    pub grip: f64,
    pub round: bool,
    /// Whether hovering lights anything.
    pub hover: bool,
    /// Whether instances wear their authoring chrome (`chrome.component`).
    pub component: bool,
}

impl Default for Chrome {
    fn default() -> Self {
        Chrome { accent: None, grip: 8.0, round: false, hover: true, component: true }
    }
}

/// `#rrggbb` or `#rrggbbaa` as an sRGB colour.
fn hex(v: &str) -> Option<[f32; 4]> {
    let h = v.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| b as f32 / 255.0);
    Some([byte(0)?, byte(2)?, byte(4)?, if h.len() == 8 { byte(6)? } else { 1.0 }])
}

#[derive(Resource, Default)]
pub struct ViewFocus {
    /// Each view's focus and what it offers, in the views' order.
    pub views: Vec<(String, Focus, Offer)>,
    /// The view the pointer was last over: plain keys go to it when the
    /// pointer is over none.
    pub last_panel: Option<String>,
    /// What the focus was computed from, so it is recomputed only on change.
    stamp: Option<Stamp>,
}

#[derive(PartialEq)]
struct Stamp {
    version: u64,
    selected: Vec<NodeId>,
    text: Option<(NodeId, String)>,
    points: Option<String>,
    views: Vec<(String, Option<NodeId>)>,
}

impl ViewFocus {
    /// A view's tolerances (the defaults for a view not yet focused).
    pub fn tolerances(&self, panel: Option<&str>) -> crate::bevy_app::tolerances::Tolerances {
        panel.and_then(|p| self.views.iter().find(|(v, ..)| v == p)).map(|(_, _, o)| o.tolerances).unwrap_or_default()
    }

    /// A view's grid, px (`snap.grid`; 0 for none).
    pub fn grid(&self, panel: &str) -> f64 {
        self.views.iter().find(|(v, ..)| v == panel).map_or(0.0, |(_, _, o)| o.grid)
    }

    /// A view's chrome (the defaults for a view not yet focused).
    pub fn chrome(&self, panel: &str) -> Chrome {
        self.views.iter().find(|(v, ..)| v == panel).map(|(_, _, o)| o.chrome).unwrap_or_default()
    }

    /// A view's offer, if it has been focused.
    pub fn offer(&self, panel: &str) -> Option<&Offer> {
        self.views.iter().find(|(v, ..)| v == panel).map(|(_, _, o)| o)
    }
}

/// The composition grid a 2D view's moves, resizes and new shapes snap
/// to, in its document px: `composition.grid_spacing` while the grid is
/// shown (`grid` not `off`) and `grid_snap` is on. A notation's own grid
/// (`snap.grid`, [`ViewFocus::grid`]) takes precedence where it sets one.
pub fn canvas_grid(store: &iron_document::Store, view: &crate::document_bridge::PanelView) -> Option<f64> {
    let id = view.composition.filter(|_| view.dims == Dims::Two)?;
    let get = |p: &str| store.resolved(id, p.parse().expect("composition leaf"));
    let shown = get("composition.grid").is_some_and(|v| v.as_str().is_some_and(|m| m != "off"));
    let on = get("composition.grid_snap") == Some(iron_document::Value::Bool(true));
    let g = get("composition.grid_spacing")?.as_f64()?;
    (shown && on && g.is_finite() && g > 0.0).then_some(g)
}

/// Turn plain letter keys into tool picks (`tool:<id>`), and the shell's
/// chords (`chord:mod+alt+g`) into command ids (`wrap`), through the keymaps
/// of the view under the pointer (or the last one it was over; before any,
/// the first view that binds the key). A key or chord that view does not
/// bind is dropped. Keys with modifiers, named keys (`escape`, `enter`, …)
/// and commands already named pass untouched, in their order.
pub fn route_tool_keys(hits: Res<crate::PointerHits>, mut focus: ResMut<ViewFocus>, mut keys: ResMut<ToolKeys>) {
    if let Some(p) = &hits.panel
        && focus.last_panel.as_deref() != Some(p.as_str())
    {
        focus.last_panel = Some(p.clone());
    }
    if keys.0.is_empty() {
        return;
    }
    let focus = &*focus;
    let lookup = |bound: &dyn Fn(&Offer) -> Option<String>| -> Option<String> {
        match &focus.last_panel {
            Some(p) => focus.views.iter().find(|(v, ..)| v == p).and_then(|(_, _, o)| bound(o)),
            None => focus.views.iter().find_map(|(_, _, o)| bound(o)),
        }
    };
    let tool_for = |key: &str| lookup(&|o: &Offer| contexts::keymap(&o.keys).into_iter().find(|(k, _)| *k == key).map(|(_, t)| t.to_owned()));
    let command_for = |chord: &str| lookup(&|o: &Offer| iron_document::commands::keymap(&o.commands).into_iter().find(|(c, _)| c == chord).map(|(_, id)| id.to_owned()));
    keys.0.retain_mut(|(key, m)| {
        if let Some(chord) = key.strip_prefix("chord:") {
            return match iron_document::commands::chord(chord).ok().and_then(|c| command_for(&c)) {
                Some(id) => {
                    *key = id;
                    *m = 0;
                    true
                }
                None => false,
            };
        }
        if *m != 0 || key.chars().count() != 1 {
            return true;
        }
        match tool_for(key) {
            Some(tool) => {
                *key = format!("tool:{tool}");
                true
            }
            None => false,
        }
    });
}

/// Recompute each view's focus when what it depends on changed, and tell
/// the shell what each view offers when that changed.
pub fn view_focus_system(
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    sel: Res<crate::SelectionState>,
    select: Res<SelectTool>,
    edit: Res<PathEdit>,
    text: Res<TextTool>,
    mut focus: ResMut<ViewFocus>,
    events: Res<HostEvents>,
) {
    let doc = store.0.document();
    let stamp = Stamp {
        version: doc.version().0,
        selected: sel.selected.clone(),
        text: text.edit.as_ref().map(|e| (e.id, e.panel.clone())),
        points: edit.reach.as_ref().map(|r| r.panel.clone()),
        views: views.0.iter().map(|v| (v.panel.clone(), v.composition)).collect(),
    };
    if focus.stamp.as_ref() == Some(&stamp) {
        return;
    }
    let r = Resolver::new(doc, store.0.contexts());
    let global = r.workspace_values().get(Field::COMMAND_KEYS).to_owned();
    let mut next = Vec::new();
    for view in &views.0 {
        let reach = select.reach.values().find(|r| r.panel == view.panel && sel.selected.contains(&r.node()));
        let mut world = view.composition;
        let mut scope: Vec<NodeId> = reach.map(|r| r.chain[..r.chain.len() - 1].to_vec()).unwrap_or_default();
        // Inside a top-level 2D embed, tools create into what it shows
        // (`pen::drop_target`): that is the world.
        if let Some(&first) = scope.first()
            && nest::is_embed(doc, first)
            && let Some(shown) = composition::embedded(doc, first).filter(|&s| composition::dims(doc, s) == Some(Dims::Two))
        {
            world = Some(shown);
            scope.remove(0);
        }
        let mut states = Vec::new();
        if text.edit.as_ref().is_some_and(|e| e.panel == view.panel) {
            states.push("editing-text");
        }
        if edit.reach.as_ref().is_some_and(|r| r.panel == view.panel) {
            states.push("editing-points");
        }
        if world.is_some_and(|w| doc.children(w).iter().any(|&c| iron_document::expand::main_definition(doc, c).is_some())) {
            states.push("main");
        }
        let f = Focus { world, scope, states };
        let v = r.view_values(&f);
        let num = |f: Field, d: f64| v.get(f).parse::<f64>().unwrap_or(d);
        let tolerances = crate::bevy_app::tolerances::Tolerances { slop: num(Field::SLOP, 3.0) as f32, double_click: num(Field::DOUBLE_CLICK, 400.0) / 1000.0, hit: num(Field::HIT, 1.0) };
        let accent = v.get(Field::ACCENT);
        let chrome = Chrome {
            // The registry's default is the box's blue; only another colour
            // replaces the built-in blues, so the default look is unchanged.
            accent: (accent != Field::ACCENT.spec().options[0]).then(|| hex(accent)).flatten(),
            grip: num(Field::GRIP, 8.0),
            round: v.get(Field::GRIP_SHAPE) == "round",
            hover: v.get(Field::HOVER) != "off",
            component: v.get(Field::COMPONENT) != "off",
        };
        let offer = Offer {
            palette: v.get(Field::PALETTE).to_owned(),
            keys: v.get(Field::KEYS).to_owned(),
            commands: iron_document::commands::merge_keymaps(&global, v.get(Field::COMMAND_KEYS)),
            menu: v.get(Field::MENU).to_owned(),
            tolerances,
            chrome,
            grid: num(Field::GRID, 0.0).max(0.0),
        };
        if focus.views.iter().find(|(p, ..)| *p == view.panel).is_none_or(|(_, _, o)| (&o.palette, &o.keys, &o.commands) != (&offer.palette, &offer.keys, &offer.commands)) {
            events.push(EngineEvent::Focus { panel: view.panel.clone(), palette: offer.palette.clone(), keys: offer.keys.clone(), commands: offer.commands.clone() });
        }
        next.push((view.panel.clone(), f, offer));
    }
    focus.views = next;
    focus.stamp = Some(stamp);
}
