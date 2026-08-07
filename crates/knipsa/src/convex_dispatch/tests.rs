use super::*;

fn rounded_regular_polygon(
    center_x: f64,
    center_y: f64,
    radius_x: f64,
    radius_y: f64,
    vertices: u16,
    rotation: f64,
) -> PathD {
    (0..vertices)
        .map(|index| {
            let angle = std::f64::consts::TAU * f64::from(index) / f64::from(vertices) + rotation;
            PointD::new(
                ((center_x + radius_x * angle.cos()) * 1000.0).round() / 1000.0,
                ((center_y + radius_y * angle.sin()) * 1000.0).round() / 1000.0,
            )
        })
        .collect()
}

fn canonical_paths(mut paths: PathsD) -> PathsD {
    paths = paths
        .into_iter()
        .map(|path| {
            crate::trim_collinear_d(&path, crate::PathKind::Closed)
                .expect("oracle path remains a valid closed ring")
        })
        .collect();
    for path in &mut paths {
        if area2(path) < 0.0 {
            path.reverse();
        }
        canonicalize(path);
    }
    paths.sort_by(compare_paths);
    paths
}

fn assert_paths_close(actual: PathsD, expected: PathsD) {
    let actual = canonical_paths(actual);
    let expected = canonical_paths(expected);
    assert_eq!(actual.len(), expected.len(), "ring count differs");
    for (actual_path, expected_path) in actual.iter().zip(&expected) {
        assert_eq!(actual_path.len(), expected_path.len(), "vertex count differs");
        for (actual_point, expected_point) in actual_path.iter().zip(expected_path) {
            assert!(
                (actual_point.x - expected_point.x).abs() <= 1.0e-8
                    && (actual_point.y - expected_point.y).abs() <= 1.0e-8,
                "point differs: {actual_point:?} != {expected_point:?}",
            );
        }
    }
}

fn request<'a>(
    subjects: &'a [PathD],
    clips: &'a [PathD],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> BooleanRequestD<'a> {
    BooleanRequestD { subjects, clips, clip_type, fill_rule }
}

#[test]
fn rounded_high_vertex_pair_matches_exact_oracle() {
    let subjects = [rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0)];
    let clips = [rounded_regular_polygon(30.0, 0.0, 100.0, 100.0, 64, 0.0)];
    let operations = [
        (ClipType::Intersection, 1),
        (ClipType::Union, 1),
        (ClipType::Difference, 1),
        (ClipType::Xor, 2),
    ];

    for fill_rule in [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive] {
        for (clip_type, expected_rings) in operations {
            let request = request(&subjects, &clips, clip_type, fill_rule);
            let actual = try_boolean_opd(request).expect("large convex pair should be certified");
            assert_eq!(actual.len(), expected_rings, "{fill_rule:?} {clip_type:?}");
            let exact = crate::boolean::boolean_opd_exact(request).expect("exact oracle closes");
            assert_paths_close(actual, exact);
        }
    }
}

#[test]
fn generalized_ellipses_and_vertex_counts_match_exact_oracle() {
    for vertices in [16, 32, 96] {
        let subjects = [rounded_regular_polygon(-7.0, 3.0, 130.0, 70.0, vertices, 0.071)];
        let clips = [rounded_regular_polygon(45.0, -4.0, 105.0, 88.0, vertices, 0.071)];
        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            let request = request(&subjects, &clips, clip_type, FillRule::EvenOdd);
            let actual =
                try_boolean_opd(request).expect("ordinary convex pair should be certified");
            let exact = crate::boolean::boolean_opd_exact(request).expect("exact oracle closes");
            assert_paths_close(actual, exact);
        }
    }
}

#[test]
fn unsupported_winding_cardinality_and_small_paths_fall_back() {
    let subject = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    let clip = rounded_regular_polygon(30.0, 0.0, 100.0, 100.0, 64, 0.0);
    let subjects = [subject.clone()];
    let clips = [clip.clone()];
    assert!(
        try_boolean_opd(request(&subjects, &clips, ClipType::Intersection, FillRule::Negative,))
            .is_none()
    );

    let empty: [PathD; 0] = [];
    assert!(
        try_boolean_opd(request(&empty, &clips, ClipType::Intersection, FillRule::EvenOdd,))
            .is_none()
    );
    let two_subjects = [subject.clone(), subject];
    assert!(
        try_boolean_opd(request(&two_subjects, &clips, ClipType::Intersection, FillRule::EvenOdd,))
            .is_none()
    );

    let small_subjects = [rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 12, 0.0)];
    assert!(
        try_boolean_opd(request(
            &small_subjects,
            &clips,
            ClipType::Intersection,
            FillRule::EvenOdd,
        ))
        .is_none()
    );
}

#[test]
fn ambiguous_or_non_two_crossing_topology_falls_back() {
    let subject = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    let subjects = [subject.clone()];

    let disjoint = [rounded_regular_polygon(300.0, 0.0, 100.0, 100.0, 64, 0.0)];
    let contained = [rounded_regular_polygon(0.0, 0.0, 50.0, 50.0, 64, 0.0)];
    let touching = [rounded_regular_polygon(200.0, 0.0, 100.0, 100.0, 64, 0.0)];
    let identical = [subject.clone()];
    let many_crossings =
        [rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, std::f64::consts::PI / 64.0)];

    for clips in [&disjoint, &contained, &touching, &identical, &many_crossings] {
        assert!(
            try_boolean_opd(request(&subjects, clips, ClipType::Intersection, FillRule::EvenOdd,))
                .is_none()
        );
    }
}

#[test]
fn malformed_non_convex_and_clockwise_paths_fall_back() {
    let mut subject = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    let clip = rounded_regular_polygon(30.0, 0.0, 100.0, 100.0, 64, 0.0);
    let clips = [clip];

    let valid_subject = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    let mut invalid_clip = rounded_regular_polygon(30.0, 0.0, 100.0, 100.0, 64, 0.0);
    invalid_clip[0].x = f64::NAN;
    assert!(
        try_boolean_opd(request(
            &[valid_subject],
            &[invalid_clip],
            ClipType::Intersection,
            FillRule::EvenOdd,
        ))
        .is_none()
    );

    subject.reverse();
    let subjects = [subject.clone()];
    assert!(
        try_boolean_opd(request(&subjects, &clips, ClipType::Intersection, FillRule::EvenOdd,))
            .is_none()
    );

    subject.reverse();
    subject[8] = subject[7];
    let subjects = [subject.clone()];
    assert!(!certified_positive_convex(&subjects[0]));

    subject[8] = PointD::new(0.0, 0.0);
    let subjects = [subject];
    assert!(!certified_positive_convex(&subjects[0]));

    let mut non_finite = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    non_finite[0].x = f64::NAN;
    assert!(!certified_positive_convex(&non_finite));

    let mut non_finite_last = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    non_finite_last[63].x = f64::NAN;
    assert!(!certified_positive_convex(&non_finite_last));

    let mut duplicate = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    let delta = subtract(duplicate[2], duplicate[1]);
    let length = delta.x.hypot(delta.y);
    duplicate[2] = PointD::new(
        duplicate[1].x + delta.x * 0.25e-9 / length,
        duplicate[1].y + delta.y * 0.25e-9 / length,
    );
    assert!(!certified_positive_convex(&duplicate));

    let mut out_of_range = rounded_regular_polygon(0.0, 0.0, 100.0, 100.0, 64, 0.0);
    out_of_range[0].x = MAX_COORDINATE + 1.0;
    assert!(!certified_positive_convex(&out_of_range));
}

#[test]
fn primitive_certificates_reject_contacts_and_ill_conditioning() {
    let crossing = segment_intersection(
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 10.0),
        PointD::new(0.0, 10.0),
        PointD::new(10.0, 0.0),
    )
    .expect("proper crossing");
    assert!((crossing.1 - 0.5).abs() <= f64::EPSILON);
    assert!((crossing.2 - 0.5).abs() <= f64::EPSILON);
    assert!(
        segment_intersection(
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(10.0, 1.0),
        )
        .is_none()
    );
    assert!(
        segment_intersection(
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(2.0, -1.0),
            PointD::new(2.0, 1.0),
        )
        .is_none()
    );
    assert!(collinear_edges_overlap(
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(5.0, 0.0),
        PointD::new(15.0, 0.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(0.0, 1.0),
        PointD::new(10.0, 1.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(0.0, 0.0),
        PointD::new(1.0, 0.0),
        PointD::new(2.0, 0.0),
        PointD::new(3.0, 0.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(2.0, 0.0),
        PointD::new(3.0, 0.0),
        PointD::new(0.0, 0.0),
        PointD::new(1.0, 0.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(0.0, 0.0),
        PointD::new(1.0, 0.0),
        PointD::new(0.5, 1.0),
        PointD::new(0.5, 2.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(0.0, 0.0),
        PointD::new(0.0, 1.0),
        PointD::new(0.0, 2.0),
        PointD::new(0.0, 3.0),
    ));
    assert!(!collinear_edges_overlap(
        PointD::new(0.0, 2.0),
        PointD::new(0.0, 3.0),
        PointD::new(0.0, 0.0),
        PointD::new(0.0, 1.0),
    ));
    assert!(in_unit_interval(-f64::EPSILON));
    assert!(in_unit_interval(1.0 + f64::EPSILON));
    assert!(!in_unit_interval(2.0));
}

#[test]
#[allow(clippy::too_many_lines)]
fn arc_and_angle_helpers_cover_wrap_and_direction_cases() {
    let start = Position { edge: 3, parameter: 0.25, crossing: 0 };
    let end = Position { edge: 7, parameter: 0.75, crossing: 1 };
    assert!(!position_after(start, end));
    assert!(position_after(end, start));
    assert!(position_after(Position { edge: 3, parameter: 0.5, crossing: 0 }, start,));

    assert_eq!(forward_vertex_count(Arc { start, end, ..Arc::default() }, 16,), 4,);
    assert_eq!(
        forward_vertex_count(Arc { start: end, end: start, wraps: true, ..Arc::default() }, 16,),
        12,
    );
    assert_eq!(
        forward_vertex_count(Arc { start, end: start, wraps: false, ..Arc::default() }, 16,),
        0,
    );
    assert_eq!(
        forward_vertex_count(Arc { start, end: start, wraps: true, ..Arc::default() }, 16,),
        16,
    );

    assert_eq!(compare_angle(PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)), Ordering::Less,);
    assert_eq!(compare_angle(PointD::new(0.0, -1.0), PointD::new(1.0, 0.0)), Ordering::Greater,);
    assert_eq!(compare_angle(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)), Ordering::Less,);
}

#[test]
fn robust_predicates_and_keys_reject_ambiguous_values() {
    assert_eq!(
        robust_cross_order(PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)),
        Some(Ordering::Greater),
    );
    assert_eq!(
        robust_cross_order(PointD::new(1.0, 0.0), PointD::new(0.0, -1.0)),
        Some(Ordering::Less),
    );
    assert_eq!(robust_cross_order(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)), None,);
    assert_eq!(
        key(PointD::new(1.25, -2.5)),
        Some(PointKey { x: 1_250_000_000, y: -2_500_000_000 }),
    );
    assert!(key(PointD::new(f64::INFINITY, 0.0)).is_none());
    assert!(key(PointD::new(0.0, f64::INFINITY)).is_none());
    assert!(key(PointD::new(MAX_COORDINATE + 1.0, 0.0)).is_none());
    assert!(key(PointD::new(0.0, MAX_COORDINATE + 1.0)).is_none());
}

#[test]
fn helper_guards_cover_reversed_arcs_and_stitch_failures() {
    let mut empty = Vec::new();
    canonicalize(&mut empty);
    let square = vec![
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(10.0, 10.0),
        PointD::new(0.0, 10.0),
    ];
    let crossings = [
        Crossing {
            subject_edge: 3,
            clip_edge: 0,
            subject_parameter: 0.25,
            clip_parameter: 0.25,
            ..Crossing::default()
        },
        Crossing {
            subject_edge: 0,
            clip_edge: 1,
            subject_parameter: 0.75,
            clip_parameter: 0.75,
            ..Crossing::default()
        },
    ];
    let arcs = build_arcs(&square, &square, &crossings, true).expect("synthetic arcs are valid");
    assert_eq!(arcs[0].start.edge, 0);

    let wrapped = Arc {
        start: Position { edge: 0, parameter: 0.0, crossing: 0 },
        end: Position { edge: 1, parameter: 0.0, crossing: 1 },
        ..Arc::default()
    };
    assert_eq!(next_forward_point(Arc::default(), &square, &crossings), crossings[0].point);
    assert_eq!(previous_forward_point(Arc::default(), &square, &crossings), crossings[0].point);
    assert_eq!(next_forward_point(wrapped, &square, &crossings), square[1]);
    assert_eq!(previous_forward_point(wrapped, &square, &crossings), square[1]);

    assert!(stitch_chains(&[], &square, &square, &crossings).is_none());
    assert!(
        stitch_chains(
            &[Chain { start_crossing: 0, end_crossing: 1, ..Chain::default() }],
            &square,
            &square,
            &crossings,
        )
        .is_none()
    );
    assert!(
        stitch_chains(
            &[
                Chain { start_crossing: 0, end_crossing: 1, ..Chain::default() },
                Chain { start_crossing: 1, end_crossing: 1, ..Chain::default() },
            ],
            &square,
            &square,
            &crossings,
        )
        .is_none()
    );
    assert!(stitch_chains(&[Chain::default()], &square, &square, &crossings).is_none());

    let collinear = vec![
        PointD::new(0.0, 0.0),
        PointD::new(1.0, 0.0),
        PointD::new(2.0, 0.0),
        PointD::new(3.0, 0.0),
    ];
    let zero_area_chain = Chain {
        arc: Arc {
            subject: true,
            start: Position { edge: 0, parameter: 0.0, crossing: 0 },
            end: Position { edge: 0, parameter: 0.0, crossing: 0 },
            wraps: true,
            ..Arc::default()
        },
        ..Chain::default()
    };
    assert!(stitch_chains(&[zero_area_chain], &collinear, &square, &crossings).is_none());
}
