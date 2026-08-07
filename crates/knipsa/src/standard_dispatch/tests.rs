use super::*;

fn rectangle(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> PathD {
    vec![
        PointD::new(min_x, min_y),
        PointD::new(max_x, min_y),
        PointD::new(max_x, max_y),
        PointD::new(min_x, max_y),
    ]
}

fn canonical_geometry(mut paths: PathsD) -> PathsD {
    for path in &mut paths {
        if signed_area2(path) < 0.0 {
            path.reverse();
        }
        canonicalize(path);
    }
    paths.sort_by(compare_paths);
    paths
}

fn base_result(request: BooleanRequestD<'_>) -> PathsD {
    crate::fast_dispatch::try_boolean_opd(request)
        .expect("base fast path accepts rectangle oracle")
        .expect("base fast path closes")
}

fn exact_result(request: BooleanRequestD<'_>) -> PathsD {
    crate::boolean::boolean_opd_exact(request).expect("exact oracle")
}

fn edge(
    start: (f64, f64),
    end: (f64, f64),
    start_key: (i64, i64),
    end_key: (i64, i64),
) -> DirectedEdge {
    DirectedEdge {
        start: PointD::new(start.0, start.1),
        end: PointD::new(end.0, end.1),
        start_key: PointKey { x: start_key.0, y: start_key.1 },
        end_key: PointKey { x: end_key.0, y: end_key.1 },
    }
}

#[test]
fn rectangle_pair_matches_base_for_operations_fill_rules_and_winding() {
    let cases = [
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(0.0, 0.0, 10.0, 10.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(5.0, 0.0, 15.0, 10.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(5.0, 5.0, 15.0, 15.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(2.0, 2.0, 8.0, 8.0)),
        (rectangle(2.0, 2.0, 8.0, 8.0), rectangle(0.0, 0.0, 10.0, 10.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(10.0, 0.0, 20.0, 10.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(20.0, 0.0, 30.0, 10.0)),
        (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(10.0 + 2.0e-9, 0.0, 20.0, 10.0)),
    ];
    let fill_rules = [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative];
    let clip_types = [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor];

    for (subject, clip) in cases {
        for reverse_subject in [false, true] {
            for reverse_clip in [false, true] {
                let mut subject = subject.clone();
                let mut clip = clip.clone();
                if reverse_subject {
                    subject.reverse();
                }
                if reverse_clip {
                    clip.reverse();
                }
                for fill_rule in fill_rules {
                    for clip_type in clip_types {
                        let subjects = [subject.clone()];
                        let clips = [clip.clone()];
                        let request = BooleanRequestD {
                            subjects: &subjects,
                            clips: &clips,
                            clip_type,
                            fill_rule,
                        };
                        let actual = try_rectangle_pair(request)
                            .expect("unambiguous rectangle pair uses tiny kernel");
                        assert_eq!(
                            canonical_geometry(actual),
                            canonical_geometry(base_result(request)),
                            "{fill_rule:?} {clip_type:?} reverse={reverse_subject}/{reverse_clip}",
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn wrapper_falls_back_for_non_rectangle_and_vertex_touch_topology() {
    let triangle = vec![PointD::new(1.0, 1.0), PointD::new(8.0, 1.0), PointD::new(1.0, 8.0)];
    let square = rectangle(0.0, 0.0, 10.0, 10.0);
    let subjects = [triangle];
    let clips = [square];
    let request = BooleanRequestD {
        subjects: &subjects,
        clips: &clips,
        clip_type: ClipType::Intersection,
        fill_rule: FillRule::EvenOdd,
    };
    assert!(try_rectangle_pair(request).is_none());
    assert_eq!(
        canonical_geometry(try_boolean_opd(request).unwrap().unwrap()),
        canonical_geometry(base_result(request)),
    );

    let subjects = [rectangle(0.0, 0.0, 10.0, 10.0)];
    let clips = [rectangle(10.0, 10.0, 20.0, 20.0)];
    let request = BooleanRequestD {
        subjects: &subjects,
        clips: &clips,
        clip_type: ClipType::Xor,
        fill_rule: FillRule::EvenOdd,
    };
    assert!(try_rectangle_pair(request).is_none());
    assert_eq!(
        canonical_geometry(try_boolean_opd(request).unwrap().unwrap()),
        canonical_geometry(base_result(request)),
    );
}

#[test]
fn convex_point_contact_matches_exact_oracle() {
    let left = vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0), PointD::new(5.0, 10.0)];
    let right = vec![PointD::new(10.0, 0.0), PointD::new(20.0, 0.0), PointD::new(15.0, 10.0)];
    for reverse_subject in [false, true] {
        for reverse_clip in [false, true] {
            let mut subject = left.clone();
            let mut clip = right.clone();
            if reverse_subject {
                subject.reverse();
            }
            if reverse_clip {
                clip.reverse();
            }
            let subjects = [subject];
            let clips = [clip];
            for fill_rule in [FillRule::EvenOdd, FillRule::NonZero] {
                for clip_type in
                    [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
                {
                    let request = BooleanRequestD {
                        subjects: &subjects,
                        clips: &clips,
                        clip_type,
                        fill_rule,
                    };
                    let fast = try_convex_zero_area_contact(request)
                        .expect("strictly convex point contact should be certified");
                    assert_eq!(canonical_geometry(fast), canonical_geometry(exact_result(request)));
                }
            }
        }
    }
}

#[test]
fn convex_contact_rejects_ambiguous_or_unsupported_pairs() {
    let subject = vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0), PointD::new(5.0, 10.0)];
    let shared_edge = vec![PointD::new(10.0, 0.0), PointD::new(0.0, 0.0), PointD::new(5.0, -10.0)];
    let overlap = vec![PointD::new(5.0, 0.0), PointD::new(15.0, 0.0), PointD::new(10.0, 10.0)];
    let disjoint = vec![PointD::new(20.0, 0.0), PointD::new(30.0, 0.0), PointD::new(25.0, 10.0)];
    let vertically_disjoint =
        vec![PointD::new(0.0, 20.0), PointD::new(10.0, 20.0), PointD::new(5.0, 30.0)];
    let concave = vec![
        PointD::new(10.0, 0.0),
        PointD::new(20.0, 0.0),
        PointD::new(15.0, 2.0),
        PointD::new(20.0, 10.0),
        PointD::new(10.0, 10.0),
    ];
    let aliased =
        vec![PointD::new(10.0 + 0.25e-9, 0.0), PointD::new(20.0, 0.0), PointD::new(15.0, 10.0)];
    let subjects = [subject];
    for clip in [shared_edge, overlap, disjoint, vertically_disjoint, concave, aliased] {
        let clips = [clip];
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(try_convex_zero_area_contact(request).is_none());
    }

    let clips = [vec![PointD::new(10.0, 0.0), PointD::new(20.0, 0.0), PointD::new(15.0, 10.0)]];
    let unsupported = BooleanRequestD {
        subjects: &subjects,
        clips: &clips,
        clip_type: ClipType::Xor,
        fill_rule: FillRule::Positive,
    };
    assert!(try_convex_zero_area_contact(unsupported).is_none());
    assert!(key_bounds(&[]).is_none());
    assert!(certified_strict_convex(&[]).is_none());
    let repeated_winding = vec![
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(5.0, 10.0),
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(5.0, 10.0),
    ];
    assert!(certified_strict_convex(&repeated_winding).is_none());
    let collinear = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)];
    assert!(certified_strict_convex(&collinear).is_none());
}

#[test]
fn even_odd_bow_tie_matches_exact_oracle() {
    let original = vec![
        PointD::new(0.0, 0.0),
        PointD::new(20.0, 20.0),
        PointD::new(0.0, 20.0),
        PointD::new(20.0, 0.0),
    ];
    for rotation in 0..4 {
        for reverse in [false, true] {
            let mut path = original.clone();
            path.rotate_left(rotation);
            if reverse {
                path.reverse();
            }
            let subjects = [path];
            let clips = [];
            for clip_type in [ClipType::Union, ClipType::Difference, ClipType::Xor] {
                let request = BooleanRequestD {
                    subjects: &subjects,
                    clips: &clips,
                    clip_type,
                    fill_rule: FillRule::EvenOdd,
                };
                let fast = try_even_odd_bow_tie(request).expect("certified bow tie");
                assert_eq!(canonical_geometry(fast), canonical_geometry(exact_result(request)));
                assert!(try_boolean_opd(request).is_some());
            }
        }
    }
}

#[test]
fn bow_tie_rejects_non_grid_and_unsupported_inputs() {
    let simple = [vec![
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(10.0, 10.0),
        PointD::new(0.0, 10.0),
    ]];
    let empty = [];
    let request = BooleanRequestD {
        subjects: &simple,
        clips: &empty,
        clip_type: ClipType::Union,
        fill_rule: FillRule::EvenOdd,
    };
    assert!(try_even_odd_bow_tie(request).is_none());
    assert!(
        try_even_odd_bow_tie(BooleanRequestD { fill_rule: FillRule::NonZero, ..request }).is_none()
    );
    assert!(
        try_even_odd_bow_tie(BooleanRequestD { clip_type: ClipType::Intersection, ..request })
            .is_none()
    );

    let non_grid_crossing = [vec![
        PointD::new(0.0, 0.0),
        PointD::new(1.0, 1.0),
        PointD::new(0.0, 1.0),
        PointD::new(2.0, 0.0),
    ]];
    assert!(
        try_even_odd_bow_tie(BooleanRequestD { subjects: &non_grid_crossing, ..request }).is_none()
    );

    let clips = [simple[0].clone()];
    assert!(try_even_odd_bow_tie(BooleanRequestD { clips: &clips, ..request }).is_none());
    assert!(!opposite_signs(0, 1));
    assert!(point_on_segment(
        PointKey { x: 1, y: 1 },
        PointKey { x: 0, y: 0 },
        PointKey { x: 2, y: 2 },
    ));
    let mut flat = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)];
    assert!(make_positive_triangle(&mut flat).is_none());
    let mut short = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)];
    assert!(make_positive_triangle(&mut short).is_none());

    assert!(
        proper_intersection(
            PointD::new(-1.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, -1.0),
            PointD::new(2.0, 1.0),
            [
                PointKey { x: -1, y: 0 },
                PointKey { x: 1, y: 0 },
                PointKey { x: 0, y: -1 },
                PointKey { x: 2, y: 1 },
            ],
        )
        .is_none()
    );
}

#[test]
fn exact_segment_predicates_cover_crossings_and_endpoints() {
    let point = |x, y| PointKey { x, y };
    assert!(segments_intersect(point(-1, 0), point(1, 0), point(0, -1), point(0, 1)));
    assert!(segments_intersect(point(0, 0), point(2, 0), point(1, 0), point(1, 1)));
    assert!(segments_intersect(point(0, 0), point(2, 0), point(1, 1), point(1, 0)));
    assert!(segments_intersect(point(1, 0), point(1, 1), point(0, 0), point(2, 0)));
    assert!(segments_intersect(point(1, 1), point(1, 0), point(0, 0), point(2, 0)));
    assert!(!segments_intersect(point(0, 0), point(1, 0), point(2, 1), point(2, 2)));
    assert!(!segments_intersect(point(0, 0), point(1, 0), point(2, 0), point(2, 1)));
    assert!(!segments_intersect(point(0, 0), point(1, 0), point(2, 1), point(2, 0)));
    assert!(!segments_intersect(point(2, 0), point(2, 1), point(0, 0), point(1, 0)));

    assert!(!point_on_segment(point(-1, 1), point(0, 0), point(2, 2)));
    assert!(!point_on_segment(point(3, 1), point(0, 0), point(2, 2)));
    assert!(!point_on_segment(point(1, -1), point(0, 0), point(2, 2)));
    assert!(!point_on_segment(point(1, 3), point(0, 0), point(2, 2)));
}

#[test]
fn rectangle_validation_rejects_malformed_or_ambiguous_paths() {
    assert!(axis_aligned_rectangle(&[]).is_none());
    assert!(axis_aligned_rectangle(&rectangle(0.0, 0.0, 0.0, 10.0)).is_none());
    assert!(axis_aligned_rectangle(&rectangle(0.0, 0.0, 10.0, 0.0)).is_none());
    assert!(
        axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(10.0, 0.0),
            PointD::new(0.0, 10.0),
        ])
        .is_none()
    );
    assert!(
        axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(0.0, 0.0),
            PointD::new(0.0, 10.0),
        ])
        .is_none()
    );
    assert!(
        axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.25e-9, 10.0),
        ])
        .is_none()
    );

    for index in 0..4 {
        let mut invalid = rectangle(0.0, 0.0, 10.0, 10.0);
        invalid[index].x = f64::NAN;
        assert!(axis_aligned_rectangle(&invalid).is_none());
    }
    for invalid in [
        PointD::new(f64::NAN, 0.0),
        PointD::new(0.0, f64::INFINITY),
        PointD::new(MAX_COORDINATE + 1.0, 0.0),
        PointD::new(0.0, -MAX_COORDINATE - 1.0),
    ] {
        assert!(key(invalid).is_none());
    }
    assert_eq!(
        key(PointD::new(1.25, -2.5)),
        Some(PointKey { x: 1_250_000_000, y: -2_500_000_000 }),
    );

    let subject = rectangle(0.0, 0.0, 10.0, 10.0);
    let subject_paths = [subject];
    let x_alias_clip = [rectangle(0.25e-9, 0.0, 20.0, 10.0)];
    let y_alias_clip = [rectangle(0.0, 0.25e-9, 20.0, 10.0)];
    for clips in [&x_alias_clip, &y_alias_clip] {
        let request = BooleanRequestD {
            subjects: &subject_paths,
            clips,
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(try_rectangle_pair(request).is_none());
    }
}

#[test]
fn coordinate_helpers_reject_quantized_aliases() {
    let path = rectangle(0.0, 0.0, 10.0, 10.0);
    let keys = [
        key(path[0]).unwrap(),
        key(path[1]).unwrap(),
        key(path[2]).unwrap(),
        key(path[3]).unwrap(),
    ];
    assert_eq!(keyed_coordinate_value(&path, &keys, 123, true), None);
    assert_eq!(keyed_coordinate_value(&path, &keys, 0, true), Some(0.0));
    assert_eq!(keyed_coordinate_value(&path, &keys, 0, false), Some(0.0));

    let aliased_path = [
        PointD::new(0.0, 0.0),
        PointD::new(0.25e-9, 1.0),
        PointD::new(1.0, 1.0),
        PointD::new(1.0, 0.0),
    ];
    let aliased_keys = [
        PointKey { x: 0, y: 0 },
        PointKey { x: 0, y: 1 },
        PointKey { x: 1, y: 1 },
        PointKey { x: 1, y: 0 },
    ];
    assert_eq!(keyed_coordinate_value(&aliased_path, &aliased_keys, 0, true), None);

    let zero = GridCoordinate { key: 0, value: 0.0 };
    let alias = GridCoordinate { key: 0, value: 0.25e-9 };
    let ten = GridCoordinate { key: 10, value: 10.0 };
    let (coordinates, len) = tiny_coordinates(zero, ten, zero, ten).unwrap();
    assert_eq!(len, 2);
    assert_eq!(coordinates[0].key, 0);
    assert_eq!(coordinates[1].key, 10);
    assert!(tiny_coordinates(zero, ten, alias, ten).is_none());
}

#[test]
fn fixed_boundary_and_stitch_reject_invalid_graphs() {
    let mut boundary = SmallBoundary::new();
    let empty_edge = DirectedEdge::default();
    for _ in 0..MAX_BOUNDARY_EDGES {
        assert_eq!(boundary.push(empty_edge), Some(()));
    }
    assert_eq!(boundary.as_slice().len(), MAX_BOUNDARY_EDGES);
    assert_eq!(boundary.push(empty_edge), None);
    assert_eq!(stitch_small(&[]), Some(Vec::new()));

    let open = [edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0))];
    assert!(stitch_small(&open).is_none());
    assert!(stitch_small(&vec![open[0]; MAX_BOUNDARY_EDGES + 1]).is_none());

    let multiple = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (1.0, 1.0), (1, 0), (1, 1)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
    ];
    assert!(stitch_small(&multiple).is_none());

    let greater_turn = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
        edge((1.0, 0.0), (1.0, 1.0), (1, 0), (1, 1)),
    ];
    assert!(stitch_small(&greater_turn).is_none());

    let equal_turn = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
    ];
    assert!(stitch_small(&equal_turn).is_none());

    let mut full_boundary = SmallBoundary::new();
    for _ in 0..MAX_BOUNDARY_EDGES {
        assert!(full_boundary.push(DirectedEdge::default()).is_some());
    }
    assert!(
        finish_rectangle_boundary(
            full_boundary,
            GridCoordinate { key: 0, value: 0.0 },
            &[GridCoordinate { key: 0, value: 0.0 }, GridCoordinate { key: 1, value: 1.0 }],
            &[false],
            &[true],
        )
        .is_none()
    );

    let lollipop = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
        edge((2.0, 0.0), (1.0, 0.0), (2, 0), (1, 0)),
    ];
    assert!(stitch_small(&lollipop).is_none());

    let two_edge_cycle = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (0.0, 0.0), (1, 0), (0, 0)),
    ];
    assert!(stitch_small(&two_edge_cycle).is_none());

    let collinear_cycle = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (2.0, 0.0), (1, 0), (2, 0)),
        edge((2.0, 0.0), (0.0, 0.0), (2, 0), (0, 0)),
    ];
    assert!(stitch_small(&collinear_cycle).is_none());
}

#[test]
fn stitch_handles_multiple_loops_and_trim_failure() {
    let loops = [
        edge((0.0, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (1.0, 1.0), (1, 0), (1, 1)),
        edge((1.0, 1.0), (0.0, 0.0), (1, 1), (0, 0)),
        edge((2.0, 0.0), (3.0, 0.0), (2, 0), (3, 0)),
        edge((3.0, 0.0), (3.0, 1.0), (3, 0), (3, 1)),
        edge((3.0, 1.0), (2.0, 0.0), (3, 1), (2, 0)),
    ];
    assert_eq!(stitch_small(&loops).unwrap().len(), 2);

    let invalid = [
        edge((f64::NAN, 0.0), (1.0, 0.0), (0, 0), (1, 0)),
        edge((1.0, 0.0), (1.0, 1.0), (1, 0), (1, 1)),
        edge((1.0, 1.0), (f64::NAN, 0.0), (1, 1), (0, 0)),
    ];
    assert!(stitch_small(&invalid).is_none());
}

#[test]
fn ordering_helpers_and_request_cardinality_branches_are_covered() {
    let mut empty: PathD = Vec::new();
    canonicalize(&mut empty);
    let short = vec![PointD::new(0.0, 0.0)];
    let long = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)];
    let shifted = vec![PointD::new(1.0, 0.0)];
    assert_eq!(compare_paths(&short, &short), Ordering::Equal);
    assert_eq!(compare_paths(&short, &long), Ordering::Less);
    assert_eq!(compare_paths(&shifted, &short), Ordering::Greater);
    assert_eq!(compare_angle(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)), Ordering::Less);
    assert_eq!(compare_angle(PointD::new(1.0, 0.0), PointD::new(1.0, 0.0)), Ordering::Equal);

    let rectangle = rectangle(0.0, 0.0, 10.0, 10.0);
    let subjects = [rectangle.clone(), rectangle.clone()];
    let clips = [rectangle.clone()];
    let request = BooleanRequestD {
        subjects: &subjects,
        clips: &clips,
        clip_type: ClipType::Union,
        fill_rule: FillRule::EvenOdd,
    };
    assert!(try_rectangle_pair(request).is_none());

    let subjects = [rectangle];
    let clips: [PathD; 0] = [];
    let request = BooleanRequestD {
        subjects: &subjects,
        clips: &clips,
        clip_type: ClipType::Union,
        fill_rule: FillRule::EvenOdd,
    };
    assert!(try_rectangle_pair(request).is_none());
}
