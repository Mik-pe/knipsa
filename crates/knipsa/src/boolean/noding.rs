//! Exact coordinate order becomes integer ranks only for broad-phase comparisons.
//! Intersections and topology always use the original rational coordinates.

use super::{Edge, Rational, split_edge_pair};
use crate::spatial::{Bounds, visit_pairs};

pub(super) fn node_edges(edges: &[Edge], parameters: &mut [Vec<Rational>]) {
    let bounds = ranked_bounds(edges);
    // This visitor cannot reject a pair; Option is used by the certificate caller.
    let _ = visit_pairs(bounds.into_iter(), |first, second| {
        let (before, after) = parameters.split_at_mut(second);
        split_edge_pair(&edges[first], &edges[second], &mut before[first], &mut after[0]);
        Some(())
    });
}

fn ranked_bounds(edges: &[Edge]) -> Vec<Bounds> {
    let mut bounds = vec![(0, 0, 0, 0); edges.len()];
    let mut coordinates = Vec::with_capacity(edges.len() * 2);
    for x_axis in [true, false] {
        coordinates.clear();
        for (index, edge) in edges.iter().enumerate() {
            let (minimum, maximum) =
                if x_axis { (&edge.min_x, &edge.max_x) } else { (&edge.min_y, &edge.max_y) };
            coordinates.push((minimum, index, false));
            coordinates.push((maximum, index, true));
        }
        coordinates.sort_unstable_by(|left, right| left.0.cmp(right.0));
        let mut rank = 0_i64;
        for (position, &(value, edge, maximum)) in coordinates.iter().enumerate() {
            if position > 0 && coordinates[position - 1].0 != value {
                // There are at most twice as many ranks as already allocated Edges.
                rank += 1;
            }
            match (x_axis, maximum) {
                (true, false) => bounds[edge].0 = rank,
                (false, false) => bounds[edge].1 = rank,
                (true, true) => bounds[edge].2 = rank,
                (false, true) => bounds[edge].3 = rank,
            }
        }
    }
    bounds
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::boolean::ExactPoint;
    use crate::spatial::boxes_touch_or_overlap64;

    fn point(x: i64, y: i64) -> ExactPoint {
        ExactPoint::new(Rational::from_i64(x), Rational::from_i64(y))
    }

    fn check_nodes(edges: &[Edge]) {
        let bounds = ranked_bounds(edges);
        let mut expected_pairs = BTreeSet::new();
        let mut expected = vec![vec![Rational::zero(), Rational::one()]; edges.len()];
        for first in 0..edges.len() {
            for second in first + 1..edges.len() {
                let a = &edges[first];
                let b = &edges[second];
                let overlaps = !(a.max_x < b.min_x
                    || b.max_x < a.min_x
                    || a.max_y < b.min_y
                    || b.max_y < a.min_y);
                assert_eq!(overlaps, boxes_touch_or_overlap64(bounds[first], bounds[second]));
                if overlaps {
                    expected_pairs.insert((first, second));
                }
                let (before, after) = expected.split_at_mut(second);
                split_edge_pair(a, b, &mut before[first], &mut after[0]);
            }
        }
        let mut actual_pairs = BTreeSet::new();
        let _ = visit_pairs(bounds.into_iter(), |first, second| {
            assert!(actual_pairs.insert((first, second)), "duplicate pair");
            Some(())
        });
        assert_eq!(actual_pairs, expected_pairs);
        let mut actual = vec![vec![Rational::zero(), Rational::one()]; edges.len()];
        node_edges(edges, &mut actual);
        for (actual, expected) in actual.iter_mut().zip(&mut expected) {
            actual.sort();
            actual.dedup();
            expected.sort();
            expected.dedup();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn indexed_splits_match_exhaustive_degenerate_and_dense_arrangements() {
        check_nodes(&[]);
        let mut edges = vec![
            Edge::new(point(0, 0), point(8, 0)),
            Edge::new(point(4, 0), point(12, 0)),
            Edge::new(point(8, 0), point(0, 0)),
            Edge::new(point(4, -4), point(4, 4)),
            Edge::new(point(0, 0), point(4, 4)),
            Edge::new(point(4, 4), point(8, 0)),
            Edge::new(point(4, 0), point(4, 8)),
            Edge::new(point(12, 0), point(16, 0)),
        ];
        check_nodes(&edges[..1]);
        check_nodes(&edges);
        let mut seed = 0x739d_0383_182e_da55_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            i64::try_from((seed >> 32) % 33).unwrap() - 16
        };
        for _ in 0..64 {
            let start = point(next(), next());
            let end = point(next(), next() + 33);
            edges.push(Edge::new(start, end));
        }
        check_nodes(&edges);
        edges.reverse();
        check_nodes(&edges);
    }

    #[test]
    fn ranks_preserve_full_domain_and_subnormal_contacts_without_float_conversion() {
        let tiny = Rational::from_f64(f64::from_bits(1)).unwrap();
        let large = Rational::from_f64(f64::MAX).unwrap();
        let coordinates = [
            large.neg(),
            Rational::from_i64(i64::MIN),
            tiny.neg(),
            Rational::zero(),
            tiny,
            Rational::from_i64(i64::MAX),
            large,
        ];
        let mut edges = Vec::new();
        for x in coordinates.windows(2) {
            edges.push(Edge::new(
                ExactPoint::new(x[0].clone(), Rational::zero()),
                ExactPoint::new(x[1].clone(), Rational::zero()),
            ));
        }
        check_nodes(&edges);
        let bounds = ranked_bounds(&edges);
        for adjacent in bounds.windows(2) {
            assert_eq!(adjacent[0].2, adjacent[1].0);
            assert!(adjacent[0].0 < adjacent[0].2);
        }
    }

    #[test]
    fn sparse_long_rows_have_no_candidates_on_either_axis() {
        for transpose in [false, true] {
            let mut edges = (0..128)
                .map(|row| {
                    if transpose {
                        Edge::new(point(row * 3, -100), point(row * 3, 100))
                    } else {
                        Edge::new(point(-100, row * 3), point(100, row * 3))
                    }
                })
                .collect::<Vec<_>>();
            check_nodes(&edges);
            let mut pairs = Vec::new();
            let mut visit = |first, second| {
                pairs.push((first, second));
                Some(())
            };
            let _ = visit_pairs(ranked_bounds(&edges).into_iter(), &mut visit);
            edges.push(edges[0].clone());
            let _ = visit_pairs(ranked_bounds(&edges).into_iter(), &mut visit);
            assert_eq!(pairs, [(0, 128)]);
            check_nodes(&edges);
        }
    }
}
