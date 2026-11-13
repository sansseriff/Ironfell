//! What a press, a double-click and Esc reach, as the contexts say
//! (plans/specs/affordances.md §5.1, phase A1). The select tool asks these;
//! no node type is named here.

use crate::contexts::{Field, Resolver};
use crate::id::NodeId;
use crate::order::OrderKey;
use crate::table::Area;

/// Which node along `chain` a press selects: the outermost one whose
/// container is not open, or the hit node itself (`deep`, Cmd). A `through`
/// node is passed over; a `window` at the top of the view is open, as is any
/// container `open(i)` says the selection is inside.
pub fn target(r: &Resolver, chain: &[NodeId], open: &dyn Fn(usize) -> bool, deep: bool) -> usize {
    if deep {
        return chain.len() - 1;
    }
    for i in 0..chain.len() {
        let v = r.values(chain, i);
        if v.is(Field::SELECT, "through") && i + 1 < chain.len() {
            continue;
        }
        let edited = r.editing.is_some() && crate::expand::main_definition(r.doc, chain[i]) == r.editing;
        if !((i == 0 && v.is(Field::SELECT, "window")) || open(i) || edited) {
            return i;
        }
    }
    chain.len() - 1
}

/// Where entering `chain[k]` lands: its child along the chain, past any
/// `through` nodes.
pub fn enter(r: &Resolver, chain: &[NodeId], k: usize) -> usize {
    let mut k = k + 1;
    while k + 1 < chain.len() && r.values(chain, k).is(Field::SELECT, "through") {
        k += 1;
    }
    k
}

/// The length of the chain that leaving its last node selects: its
/// container, past any `through` nodes. At least 1.
pub fn up(r: &Resolver, chain: &[NodeId]) -> usize {
    let mut n = chain.len().saturating_sub(1);
    while n > 1 && r.values(chain, n - 1).is(Field::SELECT, "through") {
        n -= 1;
    }
    n
}

/// What a key does to a selection inside a container that navigates
/// (`keys.nav`, ARIA's composites, affordances.md §5.7).
#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    /// Select this chain instead.
    Select(Vec<NodeId>),
    /// Move the node to `order` among its siblings (Alt with an arrow in a
    /// list).
    Reorder { id: NodeId, order: OrderKey },
    /// Edit the chain's node in place (Enter on a grid's cell).
    Edit(Vec<NodeId>),
    /// Add a row after a grid's last (Tab past its last cell).
    AddRow(NodeId),
    /// Nothing moves, and the key goes no further (an edge, or an arrow
    /// across a list's axis).
    Stay,
}

/// What `key` (`arrowleft`, `tab`, `enter`, …) does to `chain`'s node, by
/// its container's `keys.nav`; `None` when the container does not navigate
/// (the key keeps its free-canvas meaning: nudge, cycle, enter). `areas`
/// gives a grid's placed cells (`table::Grid::cells`).
pub fn key(r: &Resolver, chain: &[NodeId], key: &str, shift: bool, alt: bool, areas: &dyn Fn(NodeId) -> Option<Vec<Area>>) -> Option<KeyAction> {
    let i = chain.len().checked_sub(1)?;
    // The effective container, past `through` nodes.
    let c = (0..i).rev().find(|&k| !r.values(chain, k).is(Field::SELECT, "through"))?;
    let nav = r.values(chain, c).get(Field::NAV).to_owned();
    let node = chain[i];
    match nav.as_str() {
        "list-x" | "list-y" => {
            let (back, forth) = if nav == "list-x" { ("arrowleft", "arrowright") } else { ("arrowup", "arrowdown") };
            let across = if nav == "list-x" { ["arrowup", "arrowdown"] } else { ["arrowleft", "arrowright"] };
            let items: Vec<NodeId> = r.doc.children(chain[c]).iter().copied().filter(|&k| {
                let mut ch = chain[..=c].to_vec();
                ch.push(k);
                r.values(&ch, c + 1).selectable()
            }).collect();
            let at = items.iter().position(|&k| k == node)?;
            let step: isize = if key == back { -1 } else if key == forth { 1 } else if across.contains(&key) { return Some(KeyAction::Stay) } else { return None };
            let to = at as isize + step;
            if to < 0 || to >= items.len() as isize {
                return Some(KeyAction::Stay);
            }
            let to = to as usize;
            if alt {
                let order = |k: NodeId| r.doc.live(k).map(|n| n.order.clone());
                // Past the neighbour: between it and the one beyond.
                let (a, b) = if step > 0 { (order(items[to]), items.get(to + 1).and_then(|&k| order(k))) } else { (to.checked_sub(1).and_then(|p| order(items[p])), order(items[to])) };
                let order = OrderKey::between(a.as_ref(), b.as_ref()).ok()?;
                return Some(KeyAction::Reorder { id: node, order });
            }
            let mut next = chain[..=c].to_vec();
            next.push(items[to]);
            Some(KeyAction::Select(next))
        }
        "grid" => {
            let cells = areas(chain[c])?;
            let here = cells.iter().find(|a| a.id == node)?;
            let to_chain = |a: &Area| {
                let mut next = chain[..=c].to_vec();
                let mut up = Vec::new();
                let mut cur = r.doc.live(a.id).and_then(|n| n.parent);
                while let Some(p) = cur.filter(|&p| p != chain[c]) {
                    up.push(p);
                    cur = r.doc.live(p).and_then(|n| n.parent);
                }
                next.extend(up.into_iter().rev());
                next.push(a.id);
                next
            };
            let covering = |col: isize, row: isize| cells.iter().find(|a| col >= a.col as isize && col < (a.col + a.span_x) as isize && row >= a.row as isize && row < (a.row + a.span_y) as isize);
            let (col, row) = (here.col as isize, here.row as isize);
            let target = match key {
                "arrowleft" => covering(col - 1, row),
                "arrowright" => covering(col + here.span_x as isize, row),
                "arrowup" => covering(col, row - 1),
                "arrowdown" => covering(col, row + here.span_y as isize),
                "tab" => {
                    let mut order: Vec<&Area> = cells.iter().collect();
                    order.sort_by_key(|a| (a.row, a.col));
                    let at = order.iter().position(|a| a.id == node)?;
                    match (shift, at) {
                        (true, 0) => return Some(KeyAction::Stay),
                        (true, _) => Some(order[at - 1]),
                        (false, _) if at + 1 == order.len() => return Some(KeyAction::AddRow(chain[c])),
                        (false, _) => Some(order[at + 1]),
                    }
                }
                "enter" if !shift && r.values(chain, i).is(Field::EDIT, "text") => return Some(KeyAction::Edit(chain.to_vec())),
                _ => return None,
            };
            Some(target.map_or(KeyAction::Stay, |a| KeyAction::Select(to_chain(a))))
        }
        _ => None,
    }
}
