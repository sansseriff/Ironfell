//! Clustering: terms that share a variable solve together, terms that do
//! not are independent (constraint-system.md §5.3). Union-find over
//! variables, iterated in index order so the grouping is deterministic.

use crate::Term;

/// Groups of term indices connected through shared variables, each sorted,
/// ordered by their smallest term. A term with no variables is its own
/// group.
pub fn clusters(terms: &[Term]) -> Vec<Vec<usize>> {
    let n = terms.iter().flat_map(|t| t.vars.iter().copied()).max().map_or(0, |m| m + 1);
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in terms {
        if let Some(&first) = t.vars.first() {
            for &v in &t.vars[1..] {
                let (a, b) = (find(&mut parent, first), find(&mut parent, v));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    // Group terms by their root variable, keeping first-seen order.
    let mut roots: Vec<usize> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, t) in terms.iter().enumerate() {
        match t.vars.first() {
            Some(&v) => {
                let r = find(&mut parent, v);
                match roots.iter().position(|&x| x == r) {
                    Some(k) => groups[k].push(i),
                    None => {
                        roots.push(r);
                        groups.push(vec![i]);
                    }
                }
            }
            None => groups.push(vec![i]),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tier;

    #[test]
    fn terms_group_by_shared_variables() {
        let noop = |_: &[f64], _: &mut [f64]| {};
        let terms = vec![
            Term::new(Tier::Required, vec![0, 1], 1, noop),
            Term::new(Tier::Required, vec![4, 5], 1, noop),
            Term::new(Tier::Required, vec![1, 2], 1, noop),
            Term::new(Tier::Strong, vec![2], 1, noop),
            Term::new(Tier::Required, vec![], 1, noop),
            Term::new(Tier::Required, vec![5, 0], 1, noop),
        ];
        // Term 5 joins the two groups through variables 5 and 0.
        assert_eq!(clusters(&terms), vec![vec![0, 1, 2, 3, 5], vec![4]]);
        assert_eq!(clusters(&terms[..4]), vec![vec![0, 2, 3], vec![1]]);
    }
}
