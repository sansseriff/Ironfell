//! Interned field names (entity-composition-model.md §4.1).
//!
//! `LeafPath` is `Copy` and names its field with a `&'static str`, which is
//! right for the compile-time facets. Fields of a `params` facet
//! are known only at run time, from a definition in the document. Rather
//! than a second field representation (`Field::Sym`), run-time names are
//! interned to a deduplicated `&'static str`: the same `LeafPath` shape, the
//! same `Copy`, equality by content, and files keep storing strings. The
//! cost is one leaked allocation per distinct name over the process's life,
//! which for parameter names is nothing.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

/// The one `&'static str` for `name`, allocated on first sight.
pub fn intern(name: &str) -> &'static str {
    let set = INTERNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut set = set.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = set.get(name) {
        return s;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    set.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_idempotent_by_content() {
        let a = intern("waist");
        let b = intern(&String::from("waist"));
        assert_eq!(a, b);
        assert!(std::ptr::eq(a, b), "one allocation per name");
        assert_ne!(intern("waist"), intern("waist2"));
    }
}
