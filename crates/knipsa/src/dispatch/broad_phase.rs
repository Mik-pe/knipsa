//! Integer-only broad phase for the direct-ring certificate.

use super::{boxes_touch_or_overlap64, edges_intersect64, path_bounds64};
use crate::{Path64, Point64};

type Bounds = (i64, i64, i64, i64);
const LEAF_CAPACITY: usize = 8;

pub(super) fn certify_paths(paths: &[Path64]) -> Option<()> {
    if paths.len() > 1 {
        visit_pairs(
            paths.iter().map(|path| path_bounds64(path).expect("normalized nonempty ring")),
            |_, _| None,
        )?;
    }
    for path in paths {
        visit_pairs(
            path.iter().enumerate().map(|(index, &start)| {
                edge_bounds(start, path[(index + 1) % path.len()])
            }),
            |first, second| certify_edge_pair(path, first, second),
        )?;
    }
    Some(())
}

fn edge_bounds(start: Point64, end: Point64) -> Bounds {
    (start.x.min(end.x), start.y.min(end.y), start.x.max(end.x), start.y.max(end.y))
}

fn certify_edge_pair(path: &[Point64], first: usize, second: usize) -> Option<()> {
    let a = (path[first], path[(first + 1) % path.len()]);
    let b = (path[second], path[(second + 1) % path.len()]);
    let adjacent = first + 1 == second || (first == 0 && second + 1 == path.len());
    // An endpoint-only contact is harmless only between adjacent edges.
    if !adjacent && (a.0 == b.0 || a.0 == b.1 || a.1 == b.0 || a.1 == b.1) {
        return None;
    }
    (!edges_intersect64(a, b)?).then_some(())
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    bounds: Bounds,
    id: usize,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Bounds,
    start: usize,
    end: usize,
    // Left child follows its parent; zero denotes a leaf, never a child.
    right: usize,
}

struct Tree<'a> {
    entries: &'a [Entry],
    nodes: Vec<Node>,
}

fn visit_pairs(
    bounds: impl ExactSizeIterator<Item = Bounds>,
    mut visit: impl FnMut(usize, usize) -> Option<()>,
) -> Option<()> {
    let len = bounds.len();
    if len <= LEAF_CAPACITY {
        let mut entries = [Entry { bounds: (0, 0, 0, 0), id: 0 }; LEAF_CAPACITY];
        for (entry, (id, bounds)) in entries.iter_mut().zip(bounds.enumerate()) {
            *entry = Entry { bounds, id };
        }
        return visit_leaf(&entries[..len], &mut visit);
    }
    let mut entries = bounds.enumerate().map(|(id, bounds)| Entry { bounds, id }).collect::<Vec<_>>();
    let mut nodes = Vec::with_capacity(len / (LEAF_CAPACITY / 2));
    build(&mut entries, &mut nodes, 0, len);
    Tree { entries: &entries, nodes }.visit_same(0, &mut visit)
}

fn build(entries: &mut [Entry], nodes: &mut Vec<Node>, start: usize, end: usize) -> usize {
    let bounds = entries[start + 1..end].iter().fold(entries[start].bounds, |a, b| {
        (a.0.min(b.bounds.0), a.1.min(b.bounds.1), a.2.max(b.bounds.2), a.3.max(b.bounds.3))
    });
    let index = nodes.len();
    nodes.push(Node { bounds, start, end, right: 0 });
    if end - start > LEAF_CAPACITY {
        let vertical = i128::from(bounds.3) - i128::from(bounds.1)
            > i128::from(bounds.2) - i128::from(bounds.0);
        let middle = start + (end - start) / 2;
        entries[start..end].select_nth_unstable_by(middle - start, |a, b| {
            center(a.bounds, vertical).cmp(&center(b.bounds, vertical)).then(a.id.cmp(&b.id))
        });
        build(entries, nodes, start, middle);
        let right = build(entries, nodes, middle, end);
        nodes[index].right = right;
    }
    index
}

fn center(bounds: Bounds, vertical: bool) -> i128 {
    // Twice the center: no division, rounding, or i64 addition overflow.
    if vertical {
        i128::from(bounds.1) + i128::from(bounds.3)
    } else {
        i128::from(bounds.0) + i128::from(bounds.2)
    }
}

fn visit_pair(
    a: Entry,
    b: Entry,
    visit: &mut impl FnMut(usize, usize) -> Option<()>,
) -> Option<()> {
    if boxes_touch_or_overlap64(a.bounds, b.bounds) {
        visit(a.id.min(b.id), a.id.max(b.id))?;
    }
    Some(())
}

fn visit_leaf(
    entries: &[Entry],
    visit: &mut impl FnMut(usize, usize) -> Option<()>,
) -> Option<()> {
    for (index, &a) in entries.iter().enumerate() {
        for &b in &entries[index + 1..] {
            visit_pair(a, b, visit)?;
        }
    }
    Some(())
}

impl Tree<'_> {
    fn visit_same(
        &self,
        index: usize,
        visit: &mut impl FnMut(usize, usize) -> Option<()>,
    ) -> Option<()> {
        let node = self.nodes[index];
        if node.right == 0 {
            return visit_leaf(&self.entries[node.start..node.end], visit);
        }
        self.visit_same(index + 1, visit)?;
        self.visit_cross(index + 1, node.right, visit)?;
        self.visit_same(node.right, visit)
    }

    fn visit_cross(
        &self,
        first: usize,
        second: usize,
        visit: &mut impl FnMut(usize, usize) -> Option<()>,
    ) -> Option<()> {
        let a = self.nodes[first];
        let b = self.nodes[second];
        if !boxes_touch_or_overlap64(a.bounds, b.bounds) {
            return Some(());
        }
        if a.right == 0 && b.right == 0 {
            for &left in &self.entries[a.start..a.end] {
                for &right in &self.entries[b.start..b.end] {
                    visit_pair(left, right, visit)?;
                }
            }
            return Some(());
        }
        if b.right == 0 || (a.right != 0 && a.end - a.start >= b.end - b.start) {
            self.visit_cross(first + 1, second, visit)?;
            self.visit_cross(a.right, second, visit)
        } else {
            self.visit_cross(first, second + 1, visit)?;
            self.visit_cross(first, b.right, visit)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn check_pairs(boxes: &[Bounds]) {
        let mut expected = BTreeSet::new();
        for (first, &a) in boxes.iter().enumerate() {
            for (second, &b) in boxes.iter().enumerate().skip(first + 1) {
                if boxes_touch_or_overlap64(a, b) {
                    expected.insert((first, second));
                }
            }
        }
        let mut actual = BTreeSet::new();
        assert_eq!(
            visit_pairs(boxes.iter().copied(), |first, second| {
                assert!(first < second);
                assert!(actual.insert((first, second)), "duplicate candidate");
                Some(())
            }),
            Some(())
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn candidates_match_exhaustive_closed_box_intersections() {
        let mut boxes = Vec::new();
        for min_x in -1..=1 {
            for max_x in min_x..=1 {
                for min_y in -1..=1 {
                    for max_y in min_y..=1 {
                        boxes.push((min_x, min_y, max_x, max_y));
                    }
                }
            }
        }
        check_pairs(&boxes);
        boxes.reverse();
        check_pairs(&boxes);
        for length in 0..=LEAF_CAPACITY + 1 {
            check_pairs(&boxes[..length]);
        }
    }

    #[test]
    fn candidates_match_seeded_random_and_full_range_boxes() {
        let mut seed = 0xe220_a839_7b1d_cdaf_u64;
        let mut coordinate = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            i64::try_from(seed % 257).unwrap() - 128
        };
        let mut boxes = (0..256)
            .map(|_| {
                let (a, b, c, d) = (coordinate(), coordinate(), coordinate(), coordinate());
                (a.min(c), b.min(d), a.max(c), b.max(d))
            })
            .collect::<Vec<_>>();
        boxes.extend([
            (i64::MIN, i64::MIN, i64::MAX, i64::MAX),
            (i64::MIN, 0, i64::MIN, 0),
            (i64::MAX, 0, i64::MAX, 0),
            (0, i64::MIN, 0, i64::MAX),
        ]);
        check_pairs(&boxes);
    }

    #[test]
    fn visitor_stops_immediately_on_a_rejected_pair() {
        let boxes = [(0, 0, 1, 1); 64];
        let mut calls = 0;
        assert_eq!(
            visit_pairs(boxes.iter().copied(), |_, _| {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn sparse_boxes_are_pruned_on_either_axis() {
        for vertical in [false, true] {
            let boxes = (0..4096)
                .map(|index| {
                    if vertical {
                        (-100, index * 3, 100, index * 3 + 1)
                    } else {
                        (index * 3, -100, index * 3 + 1, 100)
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(visit_pairs(boxes.into_iter(), |_, _| panic!("disjoint boxes")), Some(()));
        }
    }

    #[test]
    fn separated_rings_and_disjoint_collinear_supports_are_certified() {
        let paths = (0..256)
            .map(|index| {
                let x = index * 4;
                vec![
                    Point64::new(x, 0),
                    Point64::new(x + 2, 0),
                    Point64::new(x + 2, 2),
                    Point64::new(x, 2),
                ]
            })
            .collect::<Vec<_>>();
        assert_eq!(certify_paths(&paths), Some(()));
        let concave = vec![
            Point64::new(0, 0),
            Point64::new(4, 0),
            Point64::new(4, 4),
            Point64::new(3, 4),
            Point64::new(3, 1),
            Point64::new(1, 1),
            Point64::new(1, 4),
            Point64::new(0, 4),
        ];
        assert_eq!(certify_paths(&[concave]), Some(()));
    }

    #[test]
    fn crossings_contacts_overlaps_and_uncertain_arithmetic_defer() {
        let paths = [
            vec![
                Point64::new(0, 0),
                Point64::new(4, 4),
                Point64::new(0, 4),
                Point64::new(4, 0),
            ],
            vec![
                Point64::new(0, 0),
                Point64::new(-4, 3),
                Point64::new(-3, -2),
                Point64::new(0, 0),
                Point64::new(4, -1),
                Point64::new(3, 4),
            ],
            vec![
                Point64::new(0, 0),
                Point64::new(4, 0),
                Point64::new(2, 0),
                Point64::new(2, 2),
            ],
            vec![
                Point64::new(i64::MIN, i64::MIN),
                Point64::new(i64::MAX, i64::MIN),
                Point64::new(i64::MAX, i64::MAX),
                Point64::new(i64::MIN, i64::MAX),
            ],
        ];
        for path in paths {
            assert_eq!(certify_paths(&[path]), None);
        }
        let a = vec![Point64::new(0, 0), Point64::new(2, 0), Point64::new(0, 2)];
        let b = vec![Point64::new(2, 0), Point64::new(4, 0), Point64::new(2, 2)];
        assert_eq!(certify_paths(&[a, b]), None);
    }
}
