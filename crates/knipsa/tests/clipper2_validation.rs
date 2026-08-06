use std::{path::Path, process::Command};

#[test]
fn release_candidate_beats_native_clipper2() {
    let running_in_actions = std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true");
    let dedicated_branch = std::env::var("GITHUB_HEAD_REF")
        .is_ok_and(|value| value == "agent/clipper2-validation-run");
    if !running_in_actions || !dedicated_branch {
        return;
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("knipsa crate must live below the workspace root");
    let script = workspace.join("scripts/validate-clipper2-candidate.sh");
    let status = Command::new("bash")
        .arg(script)
        .arg(workspace)
        .current_dir(workspace)
        .status()
        .expect("temporary native Clipper2 validation harness must start");

    assert!(status.success(), "release candidate did not clear the Clipper2 gate");
}
