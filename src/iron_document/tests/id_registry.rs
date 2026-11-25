//! The expanded-path registry (`id.rs`, `EN-17`): one step per path, read
//! without a lock. The same path always gets the same id, whatever thread
//! asks first, and every id reads back its path, owner, root and item.
//! From the council review of `EN-17` (Opus).

use iron_document::NodeId;
use std::str::FromStr;

fn id(s: &str) -> NodeId {
    NodeId::from_str(s).unwrap()
}

fn b62(mut n: u64) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut b = Vec::new();
    while n > 0 {
        b.push(A[(n % 62) as usize]);
        n /= 62;
    }
    b.reverse();
    String::from_utf8(b).unwrap()
}

#[test]
fn the_path_registry_agrees_across_threads() {
    let threads: Vec<_> = (0..8u64)
        .map(|t| {
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                for i in 0..4_000u64 {
                    let a = (i * 7 + t) % 300 + 1;
                    let depth = 2 + (i % 4) as usize;
                    let steps: Vec<u64> = (0..depth).map(|d| (i / (d as u64 + 1)) % 50 + 1).collect();
                    let text = format!("#{}/{}", b62(a), steps[1..].iter().map(|&s| b62(s)).collect::<Vec<_>>().join("/"));
                    let mut n = id(&format!("#{}", b62(a)));
                    for &s in &steps[1..] {
                        n = NodeId::expanded(n, id(&format!("#{}", b62(s)))).unwrap();
                    }
                    assert_eq!(id(&text), n);
                    assert_eq!(n.to_string(), text);
                    assert_eq!(n.expanded_root(), Some(id(&format!("#{}", b62(a)))));
                    let path = n.path();
                    assert_eq!(path.len(), depth);
                    assert_eq!(NodeId::from_path(&path), Some(n));
                    let (owner, local) = n.expanded_parts().unwrap();
                    assert_eq!(local, *path.last().unwrap());
                    assert_eq!(owner.path(), path[..depth - 1].to_vec());
                    let key = format!("k{}", i % 13);
                    assert_eq!(NodeId::item_scope(n, &key).unwrap().item_of(n), Some(key.clone()));
                    let item = NodeId::item(n, &key, local).unwrap();
                    assert_eq!(NodeId::from_str(&item.to_string()).unwrap(), item);
                    assert_eq!(item.repeat_item().map(|(r, k, _)| (r, k)), Some((n, key)));
                    seen.push((text, n));
                }
                seen
            })
        })
        .collect();
    for (text, n) in threads.into_iter().flat_map(|h| h.join().unwrap()) {
        assert_eq!(id(&text), n);
    }
}

/// A path whose first step is node 0 is a path of two steps, not one: the
/// registry's "no owner" is not an id.
#[test]
fn a_path_through_node_zero_reads_back() {
    let n = id("#0/5");
    assert_eq!(n.to_string(), "#0/5");
    assert_eq!(n.path(), vec![id("#0"), id("#5")]);
}
