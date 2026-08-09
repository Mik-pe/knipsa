#![allow(missing_docs)]

use std::collections::HashSet;

use knipsa::{
    BooleanRequest, ClipType, ComplexityLimits, EndType, FillRule, JoinType, OffsetOptions, Path64,
    Point64, PointD, RectD, boolean_op, build_polygons64, clip_to_rect_d, offset_paths64,
    signed_area2, triangulate64,
};
use serde::Deserialize;

const CORPUS: &str = include_str!("../../../tests/fixtures/clipper2-issue-corpus-v1.json");

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    source_repository: String,
    reviewed_at: String,
    fixture_license: String,
    derivation: String,
    cases: Vec<Case>,
    excluded: Vec<ExcludedIssue>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    issue: u64,
    evidence: String,
    reference_comparison: Option<String>,
    #[serde(flatten)]
    operation: Operation,
}

#[derive(Debug, Deserialize)]
struct ExcludedIssue {
    issue: u64,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Operation {
    Boolean {
        clip_type: ClipTypeName,
        fill_rule: FillRuleName,
        subjects: Vec<Vec<[i64; 2]>>,
        clips: Vec<Vec<[i64; 2]>>,
        expect_non_empty: bool,
    },
    OpenBoolean {
        clip_type: ClipTypeName,
        fill_rule: FillRuleName,
        open_subjects: Vec<Vec<[i64; 2]>>,
        clips: Vec<Vec<[i64; 2]>>,
        min_open_paths: usize,
    },
    Offset {
        paths: Vec<Vec<[i64; 2]>>,
        delta: f64,
        join_type: JoinTypeName,
        end_type: EndTypeName,
        #[serde(default)]
        min_paths: usize,
        max_paths: Option<usize>,
    },
    RectClip {
        path: Vec<[f64; 2]>,
        rect: [f64; 4],
        min_paths: usize,
    },
    Triangulate {
        paths: Vec<Vec<[i64; 2]>>,
        min_triangles: usize,
    },
    Topology {
        subjects: Vec<Vec<[i64; 2]>>,
        min_outer_polygons: usize,
        min_total_holes: usize,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClipTypeName {
    Intersection,
    Union,
    Difference,
    Xor,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FillRuleName {
    EvenOdd,
    NonZero,
    Positive,
    Negative,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JoinTypeName {
    Square,
    Bevel,
    Round,
    Miter,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EndTypeName {
    Polygon,
    Joined,
    Butt,
    Square,
    Round,
}

#[test]
fn verified_clipper2_issue_families_remain_robust() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("valid Clipper2 issue corpus");
    validate_metadata(&corpus);

    for case in &corpus.cases {
        run_case(case);
    }
}

fn validate_metadata(corpus: &Corpus) {
    assert_eq!(corpus.schema, "knipsa-clipper2-issue-corpus-v1");
    assert_eq!(corpus.source_repository, "https://github.com/AngusJohnson/Clipper2");
    assert_eq!(corpus.reviewed_at, "2026-08-09");
    assert_eq!(corpus.fixture_license, "MIT OR Apache-2.0");
    assert!(corpus.derivation.contains("no upstream fixture or test code is copied"));

    let mut ids = HashSet::new();
    let mut included_issues = HashSet::new();
    for case in &corpus.cases {
        assert!(ids.insert(&case.id), "duplicate corpus id {}", case.id);
        assert!(case.issue > 0, "{} has no upstream issue", case.id);
        assert!(
            matches!(
                case.evidence.as_str(),
                "maintainer_confirmed" | "maintainer_fixed" | "maintainer_confirmed_fixed"
            ),
            "{} has unsupported evidence {}",
            case.id,
            case.evidence
        );
        assert!(
            case.reference_comparison.as_deref().is_none_or(|value| value == "invariant_only"),
            "{} has unsupported reference comparison",
            case.id
        );
        included_issues.insert(case.issue);
    }

    let mut excluded_issues = HashSet::new();
    for excluded in &corpus.excluded {
        assert!(excluded_issues.insert(excluded.issue), "duplicate exclusion {}", excluded.issue);
        assert!(!included_issues.contains(&excluded.issue));
        assert!(!excluded.reason.is_empty());
    }
}

fn run_case(case: &Case) {
    match &case.operation {
        Operation::Boolean { clip_type, fill_rule, subjects, clips, expect_non_empty } => {
            run_boolean(&case.id, *clip_type, *fill_rule, subjects, clips, *expect_non_empty);
        }
        Operation::OpenBoolean { clip_type, fill_rule, open_subjects, clips, min_open_paths } => {
            run_open_boolean(
                &case.id,
                *clip_type,
                *fill_rule,
                open_subjects,
                clips,
                *min_open_paths,
            );
        }
        Operation::Offset { paths: input, delta, join_type, end_type, min_paths, max_paths } => {
            run_offset(&case.id, input, *delta, *join_type, *end_type, *min_paths, *max_paths);
        }
        Operation::RectClip { path, rect, min_paths } => {
            run_rect_clip(&case.id, path, *rect, *min_paths);
        }
        Operation::Triangulate { paths: input, min_triangles } => {
            run_triangulation(&case.id, input, *min_triangles);
        }
        Operation::Topology { subjects, min_outer_polygons, min_total_holes } => {
            run_topology(&case.id, subjects, *min_outer_polygons, *min_total_holes);
        }
    }
}

fn run_boolean(
    id: &str,
    clip_type: ClipTypeName,
    fill_rule: FillRuleName,
    subjects: &[Vec<[i64; 2]>],
    clips: &[Vec<[i64; 2]>],
    expect_non_empty: bool,
) {
    let subjects = paths(subjects);
    let clips = paths(clips);
    let output =
        boolean_op(BooleanRequest::new(&subjects, &clips, clip_type.into(), fill_rule.into()))
            .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert_eq!(output.closed.is_empty(), !expect_non_empty, "{id}");
    assert_valid_rings(id, &output.closed);
}

fn run_open_boolean(
    id: &str,
    clip_type: ClipTypeName,
    fill_rule: FillRuleName,
    open_subjects: &[Vec<[i64; 2]>],
    clips: &[Vec<[i64; 2]>],
    min_open_paths: usize,
) {
    let open_subjects = paths(open_subjects);
    let clips = paths(clips);
    let request = BooleanRequest {
        closed_subjects: &[],
        open_subjects: &open_subjects,
        clips: &clips,
        clip_type: clip_type.into(),
        fill_rule: fill_rule.into(),
        limits: ComplexityLimits::DEFAULT,
    };
    let output = boolean_op(request).unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert!(output.closed.is_empty(), "{id} returned closed paths");
    assert!(output.open.len() >= min_open_paths, "{id} returned too few open paths");
    assert!(output.open.iter().all(|path| path.len() >= 2), "{id} returned a short path");
}

fn run_offset(
    id: &str,
    input: &[Vec<[i64; 2]>],
    delta: f64,
    join_type: JoinTypeName,
    end_type: EndTypeName,
    min_paths: usize,
    max_paths: Option<usize>,
) {
    let options = OffsetOptions {
        join_type: join_type.into(),
        end_type: end_type.into(),
        ..OffsetOptions::default()
    };
    let output = offset_paths64(&paths(input), delta, options)
        .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert!(output.len() >= min_paths, "{id} returned too few paths");
    if let Some(max_paths) = max_paths {
        assert!(output.len() <= max_paths, "{id} returned too many paths");
    }
    assert_valid_rings(id, &output);
}

fn run_rect_clip(id: &str, path: &[[f64; 2]], rect: [f64; 4], min_paths: usize) {
    let rectangle = RectD::new(rect[0], rect[1], rect[2], rect[3]);
    let path = path.iter().map(|[x, y]| PointD::new(*x, *y)).collect();
    let output = clip_to_rect_d(&[path], rectangle, FillRule::NonZero)
        .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert!(output.len() >= min_paths, "{id} returned too few paths");
    assert!(
        output.iter().flatten().all(|point| {
            point.x >= rectangle.min_x
                && point.x <= rectangle.max_x
                && point.y >= rectangle.min_y
                && point.y <= rectangle.max_y
        }),
        "{id} returned a point outside the clip rectangle"
    );
    assert!(output.iter().all(|ring| ring.len() >= 3), "{id} returned a short ring");
}

fn run_triangulation(id: &str, input: &[Vec<[i64; 2]>], min_triangles: usize) {
    let triangles = triangulate64(&paths(input), FillRule::NonZero, ComplexityLimits::DEFAULT)
        .unwrap_or_else(|error| panic!("{id} failed: {error}"));
    assert!(triangles.len() >= min_triangles, "{id} returned too few triangles");
    assert!(
        triangles.iter().all(|triangle| signed_area2(triangle).is_ok_and(|area| area > 0)),
        "{id} returned a degenerate or reversed triangle"
    );
}

fn run_topology(
    id: &str,
    subjects: &[Vec<[i64; 2]>],
    min_outer_polygons: usize,
    min_total_holes: usize,
) {
    let subjects = paths(subjects);
    let rings = boolean_op(BooleanRequest::new(&subjects, &[], ClipType::Union, FillRule::NonZero))
        .unwrap_or_else(|error| panic!("{id} union failed: {error}"))
        .closed;
    let polygons = build_polygons64(&rings, FillRule::NonZero, ComplexityLimits::DEFAULT)
        .unwrap_or_else(|error| panic!("{id} topology failed: {error}"));
    assert!(polygons.len() >= min_outer_polygons, "{id} lost an island");
    let hole_count = polygons.iter().map(|polygon| polygon.holes.len()).sum::<usize>();
    assert!(hole_count >= min_total_holes, "{id} lost a hole");
    assert!(polygons.iter().all(|polygon| signed_area2(&polygon.outer).is_ok_and(|area| area > 0)));
    assert!(
        polygons
            .iter()
            .flat_map(|polygon| &polygon.holes)
            .all(|hole| signed_area2(hole).is_ok_and(|area| area < 0))
    );
}

fn paths(input: &[Vec<[i64; 2]>]) -> Vec<Path64> {
    input.iter().map(|path| path64(path)).collect()
}

fn path64(input: &[[i64; 2]]) -> Path64 {
    input.iter().map(|[x, y]| Point64::new(*x, *y)).collect()
}

fn assert_valid_rings(id: &str, rings: &[Path64]) {
    assert!(
        rings.iter().all(|ring| ring.len() >= 3 && signed_area2(ring).is_ok_and(|area| area != 0)),
        "{id} returned a degenerate ring"
    );
}

impl From<ClipTypeName> for ClipType {
    fn from(value: ClipTypeName) -> Self {
        match value {
            ClipTypeName::Intersection => Self::Intersection,
            ClipTypeName::Union => Self::Union,
            ClipTypeName::Difference => Self::Difference,
            ClipTypeName::Xor => Self::Xor,
        }
    }
}

impl From<FillRuleName> for FillRule {
    fn from(value: FillRuleName) -> Self {
        match value {
            FillRuleName::EvenOdd => Self::EvenOdd,
            FillRuleName::NonZero => Self::NonZero,
            FillRuleName::Positive => Self::Positive,
            FillRuleName::Negative => Self::Negative,
        }
    }
}

impl From<JoinTypeName> for JoinType {
    fn from(value: JoinTypeName) -> Self {
        match value {
            JoinTypeName::Square => Self::Square,
            JoinTypeName::Bevel => Self::Bevel,
            JoinTypeName::Round => Self::Round,
            JoinTypeName::Miter => Self::Miter,
        }
    }
}

impl From<EndTypeName> for EndType {
    fn from(value: EndTypeName) -> Self {
        match value {
            EndTypeName::Polygon => Self::Polygon,
            EndTypeName::Joined => Self::Joined,
            EndTypeName::Butt => Self::Butt,
            EndTypeName::Square => Self::Square,
            EndTypeName::Round => Self::Round,
        }
    }
}
