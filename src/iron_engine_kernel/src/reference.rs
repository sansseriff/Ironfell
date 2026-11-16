//! The reference: every value of the program, recomputed from the roots.
//! It is what "observing everything throughout" means (spec §8.8), and what
//! the old engine does on every structural change.

use crate::model::*;
use std::collections::BTreeMap;

enum Val {
    Cell(Slot),
    Items(Vec<Vec<Val>>),
}

struct Frame {
    vals: Vec<Val>,
    item: Option<(Value, usize)>,
}

struct Look<'a> {
    frames: &'a [Frame],
    inputs: &'a [Value],
}

fn cell(v: Option<&Val>) -> Slot {
    match v {
        Some(Val::Cell(s)) => s.clone(),
        _ => Err("no such cell".into()),
    }
}

impl Reads for Look<'_> {
    fn input(&mut self, i: usize) -> Slot {
        Ok(self.inputs[i].clone())
    }
    fn local(&mut self, k: usize) -> Slot {
        cell(self.frames.last().and_then(|f| f.vals.get(k)))
    }
    fn up(&mut self, d: usize, k: usize) -> Slot {
        cell(self.frames[self.frames.len() - 1 - d].vals.get(k))
    }
    fn item_value(&mut self) -> Slot {
        Ok(self.frames.last().and_then(|f| f.item.clone()).expect("inside an item").0)
    }
    fn item_index(&mut self) -> Slot {
        Ok(Value::Num(self.frames.last().and_then(|f| f.item.clone()).expect("inside an item").1 as f64))
    }
    fn collect(&mut self, k: usize, l: usize) -> Vec<Slot> {
        match self.frames.last().and_then(|f| f.vals.get(k)) {
            Some(Val::Items(items)) => items.iter().map(|vals| cell(vals.get(l))).collect(),
            _ => Vec::new(),
        }
    }
    fn root(&mut self, entry: usize) -> Slot {
        cell(self.frames[0].vals.get(entry))
    }
}

fn owner(t: &Template, frames: &mut Vec<Frame>, inputs: &[Value], path: &Path, out: &mut BTreeMap<(Path, usize), Slot>) {
    for (i, entry) in t.entries.iter().enumerate() {
        match entry {
            Entry::Cell(e) => {
                let v = eval(e, &mut Look { frames, inputs });
                out.insert((path.clone(), i), v.clone());
                frames.last_mut().expect("a frame").vals.push(Val::Cell(v));
            }
            Entry::Collection { each, template } => {
                let each = eval(each, &mut Look { frames, inputs });
                let mut items = Vec::new();
                for (index, (key, value)) in items_of(&each).into_iter().enumerate() {
                    let mut p = path.clone();
                    p.push((i, key));
                    frames.push(Frame { vals: Vec::new(), item: Some((value, index)) });
                    owner(template, frames, inputs, &p, out);
                    items.push(frames.pop().expect("the item's frame").vals);
                }
                frames.last_mut().expect("a frame").vals.push(Val::Items(items));
            }
        }
    }
}

/// Every binding's value, keyed by (owner path, entry).
pub fn reference(t: &Template, inputs: &[Value]) -> BTreeMap<(Path, usize), Slot> {
    let mut out = BTreeMap::new();
    let mut frames = vec![Frame { vals: Vec::new(), item: None }];
    owner(t, &mut frames, inputs, &Vec::new(), &mut out);
    out
}
