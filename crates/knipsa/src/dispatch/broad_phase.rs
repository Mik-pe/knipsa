//! Integer-only broad phase for the direct-ring certificate.

use super::{cross64, edges_intersect64, path_bounds64, vector64};
use crate::spatial::{Bounds, visit_pairs};
use crate::{Path64, Point64};

pub(super) fn certify_paths(paths: &[Path64]) -> Option<()> {
    if paths.len() > 1 {
        visit_pairs(
            paths.iter().map(|path| path_bounds64(path).expect("normalized nonempty ring")),
            |_, _| None,
        )?;
    }
    for path in paths {
        visit_pairs(
            path.iter()
                .enumerate()
                .map(|(index, &start)| edge_bounds(start, path[(index + 1) % path.len()])),
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
    if adjacent {
        // Shared endpoints are guaranteed here; non-collinear supports meet only there.
        let turn = cross64(vector64(a.0, a.1), vector64(b.0, b.1))?;
        return (turn != 0).then_some(());
    }
    if a.0 == b.0 || a.0 == b.1 || a.1 == b.0 || a.1 == b.1 {
        return None;
    }
    (!edges_intersect64(a, b)?).then_some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_corner_certificate_matches_full_intersection_predicate() {
        for first_x in -2..=2 {
            for first_y in -2..=2 {
                for second_x in -2..=2 {
                    for second_y in -2..=2 {
                        let path = [
                            Point64::new(0, 0),
                            Point64::new(first_x, first_y),
                            Point64::new(second_x, second_y),
                        ];
                        for (first, second) in [(0, 1), (0, 2), (1, 2)] {
                            let intersects = edges_intersect64(
                                (path[first], path[(first + 1) % path.len()]),
                                (path[second], path[(second + 1) % path.len()]),
                            )
                            .unwrap();
                            assert_eq!(
                                certify_edge_pair(&path, first, second),
                                (!intersects).then_some(())
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn non_adjacent_contacts_defer_for_every_endpoint_orientation() {
        let origin = Point64::new(0, 0);
        let horizontal = Point64::new(4, 0);
        let vertical = Point64::new(0, 4);
        for first in [(origin, horizontal), (horizontal, origin)] {
            for second in [(origin, vertical), (vertical, origin)] {
                let mut path = [
                    first.0,
                    first.1,
                    Point64::new(8, 8),
                    second.0,
                    second.1,
                    Point64::new(-8, -8),
                ];
                assert_eq!(certify_edge_pair(&path, 0, 3), None);
                path.rotate_right(1);
                assert_eq!(certify_edge_pair(&path, 1, 4), None);
            }
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
            vec![Point64::new(0, 0), Point64::new(4, 4), Point64::new(0, 4), Point64::new(4, 0)],
            vec![
                Point64::new(0, 0),
                Point64::new(-4, 3),
                Point64::new(-3, -2),
                Point64::new(0, 0),
                Point64::new(4, -1),
                Point64::new(3, 4),
            ],
            vec![Point64::new(0, 0), Point64::new(4, 0), Point64::new(2, 0), Point64::new(2, 2)],
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
