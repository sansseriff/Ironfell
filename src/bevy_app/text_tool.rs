//! The text tool and text editing, Figma's way (plans/specs/text.md §7).
//!
//! **Making text.** T arms the tool. A click makes an auto-width text at the
//! press; a drag makes a fixed box. Either way the new node is edited at
//! once, and leaving it empty takes it away again, as in Figma.
//!
//! **Editing.** Double-click a text node (or select it and press Enter) to
//! edit it. The engine keeps the draft and the selection; the draft shows
//! through a preview on `text.content`, so bindings that read the text and
//! the node's bounds follow every keystroke and nothing enters history.
//! Leaving (Esc, a press outside, another tool) commits one transaction,
//! folded into the node's creation when the node is new, so one undo takes
//! back a typed label.
//!
//! **Keystrokes never come here as keys.** The host mirrors the draft in a
//! hidden textarea placed under the caret ([`EngineEvent::TextEdit`]), so
//! typing, IME composition, dead keys, the clipboard and word-wise motion
//! are the platform's. The host sends back the whole text and selection
//! after each change ([`TextInput::Replace`]); only motions that need the
//! canvas layout (Up and Down across wrapped lines) come as
//! [`TextInput::Motion`]. Offsets cross the boundary in UTF-16 code units,
//! the textarea's, and are bytes here.
//!
//! Text whose content is bound or driven by a signal is not editable: its
//! words come from elsewhere, and the inspector shows where.

use bevy::prelude::*;
use iron_document::component::{Fill, Font, Name as DocName, Size, Text, Transform2d};
use iron_document::composition::Dims;
use iron_document::{Actor, LeafPath, LeafStruct, NodeId, Op, Slot, TransactionInput, TypeId, Value, Version};
use iron_text::Motion;
use kurbo::{Affine, Point, Rect};

use crate::bevy_app::pen::{after_last, drop_target};
use crate::bevy_app::select::{ActiveTool, Geo, SelectTool, ToolKeys};
use crate::document_bridge::{DocumentPanelLayer, DocumentStore, GestureRequest, GestureRequests, HistoryOp, HistoryRequests, PanelViews, PendingTransactions};
use crate::host_events::{EngineEvent, HostEvents};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, VectorView, order};

/// Window px a press must travel to drag out a box instead of clicking.
const DRAG: f64 = 3.0;
/// Seconds and window px between presses that make a double-click.
const DOUBLE_CLICK: (f64, f64) = (0.4, 5.0);
const BLUE: [f32; 4] = [0.05, 0.6, 1.0, 1.0];
const SELECTION: [f32; 4] = [0.05, 0.6, 1.0, 0.25];

/// From the host's textarea; see the module docs.
#[derive(Debug, Clone)]
pub enum TextInput {
    /// The whole draft and its selection, UTF-16 offsets.
    Replace { text: String, anchor: u32, focus: u32 },
    Motion { motion: Motion, extend: bool },
    /// Style the selection (plans/specs/text.md §2.1): the properties set.
    Style(iron_text::RunStyle),
    /// Cmd+B, I, U, Shift+X: `bold`, `italic`, `underline`, `strike` on
    /// the selection, on if any of it is off.
    Toggle(String),
}

/// What a press with the tool armed will make.
struct TextPress {
    panel: String,
    container: NodeId,
    to_comp: Affine,
    start: Point,
    dragged: bool,
    /// The box being dragged out, in the container's space.
    rect: Option<Rect>,
}

/// Start editing a node: from a double-click, Enter, or a node just made.
pub struct EditRequest {
    pub id: NodeId,
    pub panel: String,
    /// The container's space → window px (without the node's own placement).
    pub base: Affine,
    /// Where the pointer was, window px: the caret goes there (a double
    /// click selects the word). `None` selects everything.
    pub at: Option<Point>,
    /// The node was made for this edit, by the transaction at this version.
    pub created: Option<Version>,
}

pub struct TextEdit {
    pub id: NodeId,
    pub panel: String,
    base: Affine,
    draft: String,
    /// The draft's style runs, carried across each edit.
    runs: Vec<iron_text::StyleRun>,
    anchor: usize,
    focus: usize,
    created: Option<Version>,
    /// A pointer drag is extending the selection.
    selecting: bool,
}

#[derive(Resource, Default)]
pub struct TextTool {
    press: Option<TextPress>,
    pub edit: Option<TextEdit>,
    pub request: Option<EditRequest>,
    /// A node made by the tool, to edit once its transaction has applied.
    made: Option<(NodeId, String, Affine)>,
    pub inputs: Vec<TextInput>,
    /// What the host was last told, so it hears only of changes.
    sent: Option<(String, usize, usize, [i32; 3])>,
    last_press: Option<(f64, Point)>,
}

impl TextTool {
    pub fn editing(&self) -> Option<NodeId> {
        self.edit.as_ref().map(|e| e.id)
    }

    /// The box a drag with the tool is making: panel, view document px →
    /// the container's space, and the box there. The select overlay draws
    /// it, as it does the rectangle tool's.
    pub fn drag_box(&self) -> Option<(&str, Affine, Rect)> {
        let p = self.press.as_ref()?;
        Some((p.panel.as_str(), p.to_comp, p.rect?))
    }

    /// Editing with characters selected (styling goes to them).
    pub fn has_range(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.anchor != e.focus)
    }
}

fn content_path() -> LeafPath {
    "text.content".parse().expect("declared leaf")
}

fn runs_path() -> LeafPath {
    "text.runs".parse().expect("declared leaf")
}

/// The style at the start of `start..end` as it shows: the run there over
/// the node's own.
fn shown_style(store: &iron_document::Store, id: NodeId, runs: &[iron_text::StyleRun], start: usize, end: usize) -> (bool, bool, bool, bool) {
    let s = iron_document::text::style(store, id);
    // Every character of the range must have it for it to count as on.
    let mut all = (true, true, true, true);
    let mut i = start;
    loop {
        let r = iron_text::runs::at(runs, i);
        all.0 &= r.weight.unwrap_or(s.weight) >= 600.0;
        all.1 &= r.italic.unwrap_or(s.italic);
        all.2 &= r.underline.unwrap_or(s.underline);
        all.3 &= r.strike.unwrap_or(s.strike);
        match runs.iter().map(|r| if r.start > i { r.start } else { r.end }).filter(|&b| b > i && b < end).min() {
            Some(next) => i = next,
            None => break,
        }
    }
    all
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

/// UTF-16 offset → byte offset in `s`, clamped to a char boundary.
pub fn byte_of(s: &str, utf16: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in s.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    s.len()
}

/// Byte offset → UTF-16 offset in `s`.
pub fn utf16_of(s: &str, byte: usize) -> u32 {
    s[..byte.min(s.len())].chars().map(|c| c.len_utf16() as u32).sum()
}

/// Whether a text node's words can be typed: its content is a constant.
fn editable(store: &iron_document::Store, id: NodeId) -> bool {
    store.document().live(id).is_some_and(|n| n.ty.name() == "text") && store.document().leaf(id, content_path()).is_some_and(|s| s.expr().is_none()) && store.reactive().signal(iron_document::Leaf { node: id, path: content_path() }).is_none()
}

#[allow(clippy::too_many_arguments)]
pub fn text_tool_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    mut active: ResMut<ActiveTool>,
    mut text: ResMut<TextTool>,
    mut keys: ResMut<ToolKeys>,
    mut sel: ResMut<crate::SelectionState>,
    select: Res<SelectTool>,
    mut store: ResMut<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    (mut gestures, mut history): (ResMut<GestureRequests>, ResMut<HistoryRequests>),
    (spatial, drawables, events, time): (Res<crate::document_bridge::SpatialScene>, Res<crate::document_bridge::Drawables>, Res<HostEvents>, Res<Time>),
) {
    let text = &mut *text;
    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let now = time.elapsed_secs_f64();

    // ---- Keys ----
    let mut rest = Vec::new();
    let mut finish = false;
    for (key, m) in std::mem::take(&mut keys.0) {
        match key.as_str() {
            "t" if m == 0 && text.edit.is_none() => *active = ActiveTool::Text,
            "escape" if text.edit.is_some() => finish = true,
            "escape" if *active == ActiveTool::Text => *active = ActiveTool::Select,
            // Enter on one selected text node edits it, everything selected.
            "enter" if m == 0 && *active == ActiveTool::Select && text.edit.is_none() => {
                let lone = match sel.selected[..] {
                    [id] if editable(&store.0, id) => select.reach.get(&id).cloned(),
                    _ => None,
                };
                match lone {
                    Some(r) => {
                        let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
                        if let (Some(view), Some(space)) = (views.get(&r.panel), geo.space(&r.chain)) {
                            text.request = Some(EditRequest { id: r.node(), panel: r.panel.clone(), base: view.affine() * space, at: None, created: None });
                        }
                    }
                    None => rest.push((key, m)),
                }
            }
            _ => rest.push((key, m)),
        }
    }
    keys.0 = rest;
    // Another tool picked while editing ends the edit.
    if text.edit.is_some() && !matches!(*active, ActiveTool::Text | ActiveTool::Select) {
        finish = true;
    }

    // A double-click on the one selected text node edits it there, its
    // word selected (the first click selected it, through the select tool).
    if *active == ActiveTool::Select && text.edit.is_none() && pointer.just_pressed_left {
        let double = text.last_press.is_some_and(|(t, p)| now - t <= DOUBLE_CLICK.0 && p.distance(screen) <= DOUBLE_CLICK.1);
        text.last_press = Some((now, screen));
        if double
            && let [id] = sel.selected[..]
            && editable(&store.0, id)
            && let Some(r) = select.reach.get(&id)
            && hits.panel.as_deref() == Some(r.panel.as_str())
            && let Some(view) = views.get(&r.panel)
        {
            let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
            if let Some(space) = geo.space(&r.chain) {
                let base = view.affine() * space;
                let local = (base * geo.world(id)).inverse() * screen;
                if iron_document::text::measure(&store.0, id).frame.contains(local) {
                    text.request = Some(EditRequest { id, panel: r.panel.clone(), base, at: Some(screen), created: None });
                    text.last_press = None;
                }
            }
        }
    }

    // A node the tool made, once it exists.
    if let Some((id, panel, base)) = text.made.take() {
        if store.0.document().live(id).is_some() {
            let created = Some(store.0.document().version());
            text.request = Some(EditRequest { id, panel, base, at: None, created });
        } else if pending.0.iter().any(|t| t.ops.iter().any(|o| matches!(o, Op::Create { id: c, .. } if *c == id))) {
            text.made = Some((id, panel, base));
        }
    }

    // ---- Start an edit ----
    if let Some(req) = text.request.take() {
        if text.edit.is_some() {
            finish_edit(text, &mut store.0, &mut gestures, &mut history, &mut sel, &events);
        }
        if editable(&store.0, req.id) {
            let draft = iron_document::text::content(&store.0, req.id);
            let (anchor, focus) = match req.at {
                Some(at) => {
                    let m = iron_document::text::measure_with(&store.0, req.id, &draft, &iron_document::text::runs(&store.0, req.id));
                    let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
                    let local = (req.base * geo.world(req.id) * Affine::translate(m.offset)).inverse() * at;
                    m.layout.word_at(local.x, local.y)
                }
                None => (0, draft.len()),
            };
            store.0.begin_gesture(&[req.id]);
            sel.selected = vec![req.id];
            *active = ActiveTool::Text;
            let runs = iron_document::text::runs(&store.0, req.id);
            text.edit = Some(TextEdit { id: req.id, panel: req.panel, base: req.base, draft, runs, anchor, focus, created: req.created, selecting: false });
            text.press = None;
        }
    }

    let Some(edit) = text.edit.as_mut() else {
        if text.sent.take().is_some() {
            events.push(EngineEvent::TextEdit { active: false, node: String::new(), text: String::new(), anchor: 0, focus: 0, caret: [0.0; 3] });
        }
        if *active == ActiveTool::Text {
            arm(text, &pointer, &hits, &views, &store.0, &drawables, &spatial, &mut pending, screen);
        } else {
            text.press = None;
        }
        return;
    };

    // ---- Editing ----
    if store.0.document().live(edit.id).is_none() {
        text.edit = None;
        store.0.cancel_gesture();
        return;
    }
    let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
    let measured = iron_document::text::measure_with(&store.0, edit.id, &edit.draft, &edit.runs);
    let to_screen = edit.base * geo.world(edit.id);
    let to_layout = (to_screen * Affine::translate(measured.offset)).inverse();
    for input in std::mem::take(&mut text.inputs) {
        match input {
            TextInput::Replace { text: t, anchor, focus } => {
                edit.anchor = byte_of(&t, anchor);
                edit.focus = byte_of(&t, focus);
                edit.runs = iron_text::runs::remap(&edit.draft, &t, &edit.runs);
                edit.draft = t;
            }
            TextInput::Style(patch) => {
                let (a, b) = (edit.anchor.min(edit.focus), edit.anchor.max(edit.focus));
                edit.runs = iron_text::runs::apply(&edit.draft, &edit.runs, a, b, &patch);
            }
            TextInput::Toggle(what) => {
                let (a, b) = (edit.anchor.min(edit.focus), edit.anchor.max(edit.focus));
                if a < b {
                    let (bold, italic, under, strike) = shown_style(&store.0, edit.id, &edit.runs, a, b);
                    let patch = match what.as_str() {
                        "bold" => iron_text::RunStyle { weight: Some(if bold { 400.0 } else { 700.0 }), ..Default::default() },
                        "italic" => iron_text::RunStyle { italic: Some(!italic), ..Default::default() },
                        "underline" => iron_text::RunStyle { underline: Some(!under), ..Default::default() },
                        "strike" => iron_text::RunStyle { strike: Some(!strike), ..Default::default() },
                        _ => iron_text::RunStyle::default(),
                    };
                    edit.runs = iron_text::runs::apply(&edit.draft, &edit.runs, a, b, &patch);
                }
            }
            TextInput::Motion { motion, extend } => {
                let m = iron_document::text::measure_with(&store.0, edit.id, &edit.draft, &edit.runs);
                (edit.anchor, edit.focus) = m.layout.motion(edit.anchor, edit.focus, motion, extend);
            }
        }
    }
    // The pointer: a press in the box places the caret (a double press
    // selects a word) and a drag extends; a press outside ends the edit and
    // is the select tool's.
    if pointer.just_pressed_left && hits.panel.as_deref() == Some(edit.panel.as_str()) {
        let local = to_screen.inverse() * screen;
        if measured.frame.contains(local) {
            let at = to_layout * screen;
            let double = text.last_press.is_some_and(|(t, p)| now - t <= DOUBLE_CLICK.0 && p.distance(screen) <= DOUBLE_CLICK.1);
            if double {
                (edit.anchor, edit.focus) = measured.layout.word_at(at.x, at.y);
            } else {
                let i = measured.layout.hit(at.x, at.y);
                edit.focus = i;
                if !pointer.modifiers.shift {
                    edit.anchor = i;
                }
                edit.selecting = true;
            }
            text.last_press = Some((now, screen));
        } else {
            finish = true;
        }
    } else if pointer.just_pressed_left {
        finish = true;
    }
    if edit.selecting && !finish {
        let at = to_layout * screen;
        edit.focus = measured.layout.hit(at.x, at.y);
        if !pointer.buttons.left {
            edit.selecting = false;
        }
    }

    if finish {
        finish_edit(text, &mut store.0, &mut gestures, &mut history, &mut sel, &events);
        *active = ActiveTool::Select;
        return;
    }

    // Show the draft (bindings that read it, and bounds, follow), and tell
    // the host where the caret is.
    if iron_document::text::content(&store.0, edit.id) != edit.draft {
        store.0.preview(edit.id, content_path(), Value::Str(edit.draft.clone()));
    }
    let runs_json = iron_text::runs::to_json(&edit.runs);
    if store.0.resolved(edit.id, runs_path()).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default() != runs_json {
        store.0.preview(edit.id, runs_path(), Value::Str(runs_json));
    }
    let m = iron_document::text::measure_with(&store.0, edit.id, &edit.draft, &edit.runs);
    let caret = (edit.base * geo_world(&store.0, &drawables, &spatial, edit.id) * Affine::translate(m.offset)).transform_rect_bbox(m.layout.caret(edit.focus));
    let caret_px = [caret.x0.round() as i32, caret.y0.round() as i32, caret.height().round() as i32];
    let state = (edit.draft.clone(), edit.anchor, edit.focus, caret_px);
    if text.sent.as_ref() != Some(&state) {
        events.push(EngineEvent::TextEdit {
            active: true,
            node: edit.id.to_string(),
            text: edit.draft.clone(),
            anchor: utf16_of(&edit.draft, edit.anchor),
            focus: utf16_of(&edit.draft, edit.focus),
            caret: [caret.x0 as f32, caret.y0 as f32, caret.height() as f32],
        });
        text.sent = Some(state);
    }
}

fn geo_world(store: &iron_document::Store, drawables: &crate::document_bridge::Drawables, spatial: &crate::document_bridge::SpatialScene, id: NodeId) -> Affine {
    Geo { store, drawables, spatial }.world(id)
}

/// The armed tool: a press, then a click or a drag, makes a text node.
#[allow(clippy::too_many_arguments)]
fn arm(
    text: &mut TextTool,
    pointer: &crate::PointerState,
    hits: &crate::PointerHits,
    views: &PanelViews,
    store: &iron_document::Store,
    drawables: &crate::document_bridge::Drawables,
    spatial: &crate::document_bridge::SpatialScene,
    pending: &mut PendingTransactions,
    screen: Point,
) {
    if pointer.just_pressed_left
        && let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two)
    {
        let geo = Geo { store, drawables, spatial };
        text.press = drop_target(&geo, hits, view).map(|(container, to_comp)| TextPress { panel: view.panel.clone(), container, to_comp, start: screen, dragged: false, rect: None });
    }
    let Some(press) = text.press.as_mut() else { return };
    press.dragged |= press.start.distance(screen) > DRAG;
    if press.dragged
        && let Some(view) = views.get(&press.panel)
    {
        let to_comp = press.to_comp * view.affine().inverse();
        press.rect = Some(Rect::from_points(to_comp * press.start, to_comp * screen));
    }
    if !pointer.just_released_left {
        return;
    }
    let Some(press) = text.press.take() else { return };
    let Some(view) = views.get(&press.panel) else { return };
    let to_comp = press.to_comp * view.affine().inverse();
    let (a, b) = (to_comp * press.start, to_comp * screen);
    let r = Rect::from_points(a, b);
    let doc = store.document();
    let id = doc.next_ids(1)[0];
    let mut t = Transform2d::default();
    t.x = n(if press.dragged { r.x0 } else { a.x });
    t.y = n(if press.dragged { r.y0 } else { a.y });
    let mut body = Text::default();
    let mut size = Size::default();
    if press.dragged {
        body.sizing = Slot::Const(Value::Enum("fixed".into()));
        size.w = n(r.width().max(8.0));
        size.h = n(r.height().max(8.0));
    }
    let mut fill = Fill::default();
    fill.color = Slot::Const(Value::Color([0.0, 0.0, 0.0, 1.0]));
    let mut name = DocName::default();
    name.text = Slot::Const(Value::Str("Text".into()));
    let op = Op::Create {
        id,
        ty: TypeId::lookup("text").expect("registered"),
        parent: Some(press.container),
        order: after_last(doc, Some(press.container)),
        components: vec![t.wrap(), body.wrap(), Font::default().wrap(), size.wrap(), fill.wrap(), name.wrap()],
    };
    pending.0.push(TransactionInput { label: "add text".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops: vec![op], exact: false });
    text.made = Some((id, press.panel.clone(), view.affine() * press.to_comp.inverse()));
}

/// Commit the draft, or take away a new node left empty.
fn finish_edit(text: &mut TextTool, store: &mut iron_document::Store, gestures: &mut GestureRequests, history: &mut HistoryRequests, sel: &mut crate::SelectionState, events: &HostEvents) {
    let Some(edit) = text.edit.take() else { return };
    text.sent = None;
    events.push(EngineEvent::TextEdit { active: false, node: String::new(), text: String::new(), anchor: 0, focus: 0, caret: [0.0; 3] });
    if edit.draft.is_empty()
        && let Some(v) = edit.created
    {
        store.cancel_gesture();
        sel.selected.clear();
        // Nothing typed: the node goes, with no trace in history when its
        // creation is still the latest edit.
        if store.document().version() == v {
            history.0.push(HistoryOp::Undo);
        } else {
            gestures.0.push(GestureRequest::Cancel);
        }
        return;
    }
    let label = "edit text".to_owned();
    gestures.0.push(match edit.created {
        Some(v) => GestureRequest::CommitMerged("add text".into(), v),
        None => GestureRequest::Commit(label),
    });
    sel.selected = vec![edit.id];
}

/// Marks the overlay entity that draws the caret and the selection.
#[derive(Component)]
pub struct TextEditOverlay;

/// The caret, the selection and the box of the text being edited, in their
/// own overlay view above the select overlay's.
pub fn draw_text_overlay(
    mut commands: Commands,
    text: Res<TextTool>,
    panel_layers: Query<(&DocumentPanelLayer, &VectorView), Without<TextEditOverlay>>,
    mut overlays: Query<(Entity, &mut DisplayList, &mut VectorView), With<TextEditOverlay>>,
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    drawables: Res<crate::document_bridge::Drawables>,
    spatial: Res<crate::document_bridge::SpatialScene>,
    dpr: Res<crate::Dpr>,
) {
    let Some(edit) = text.edit.as_ref() else {
        for (e, ..) in &overlays {
            commands.entity(e).despawn();
        }
        return;
    };
    let Some((_, panel_view)) = panel_layers.iter().find(|(l, _)| l.panel == edit.panel) else { return };
    let Ok((_, mut list, mut v)) = overlays.single_mut() else {
        let e = commands.spawn((DisplayList::default(), TextEditOverlay, *panel_view, crate::vector::Overlay)).id();
        commands.entity(e).insert(VectorLayer::screen(order::EDITOR_OVERLAY + 1).in_view(e));
        return;
    };
    if *v != *panel_view {
        *v = *panel_view;
    }
    let Some(view) = views.get(&edit.panel) else { return };
    let m = iron_document::text::measure_with(&store.0, edit.id, &edit.draft, &edit.runs);
    let node = edit.base * Geo { store: &store.0, drawables: &drawables, spatial: &spatial }.world(edit.id);
    let at = node * Affine::translate(m.offset);
    let k = dpr.0 as f64;
    let blue = peniko::Color::new(BLUE);
    list.rebuild(|b| {
        b.clipped(Affine::IDENTITY, view.rect.to_kurbo(), |b| {
            b.stroke(node, kurbo::Stroke::new(k / node.as_coeffs()[0].hypot(node.as_coeffs()[1]).max(1e-6)), blue, m.frame);
            for r in m.layout.selection(edit.anchor, edit.focus) {
                b.fill(at, peniko::Color::new(SELECTION), r);
            }
            if edit.anchor == edit.focus {
                let c = m.layout.caret(edit.focus);
                let top = at * Point::new(c.x0, c.y0);
                let bottom = at * Point::new(c.x0, c.y1);
                b.stroke(Affine::IDENTITY, kurbo::Stroke::new(1.0 * k), peniko::Color::new([0.0, 0.0, 0.0, 1.0]), kurbo::Line::new(top, bottom));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trips() {
        let s = "aé😀b";
        for (byte, unit) in [(0, 0), (1, 1), (3, 2), (7, 4), (8, 5)] {
            assert_eq!(utf16_of(s, byte), unit);
            assert_eq!(byte_of(s, unit), byte);
        }
    }
}
