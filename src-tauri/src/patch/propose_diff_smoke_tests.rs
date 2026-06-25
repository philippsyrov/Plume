//! D91: propose-diff model-quality smoke — the validate → apply → revert
//! cycle a local model's diff must survive, exercised through Plume's own
//! patch path (`validate_patch` / `apply_patch` / `revert_patch`), never a
//! reimplementation.
//!
//! Two halves:
//!   * In-container unit tests pin the cycle against hand-authored diffs in
//!     a throwaway fixture — they run in the normal suite and prove the
//!     orchestration (valid → applied → reverted; invalid → reported, disk
//!     untouched; pre-image mismatch → apply fails, disk untouched).
//!   * One `#[ignore]`d test, `qwen_propose_diff_smoke`, is the entry point
//!     the `scripts/smoke-qwen-propose-diff.sh` harness drives on a Mac: it
//!     reads a model-produced diff + a seeded fixture from the environment
//!     and runs the same cycle. It is `#[ignore]` so CI / `cargo test`
//!     never needs a live model.
//!
//! "Report invalid without failing the whole machine state": apply only
//! runs after `validate_patch` passes, and `apply_patch` is all-or-nothing
//! with rollback + a pre-apply checkpoint, so an invalid or non-applying
//! model diff leaves the fixture exactly as seeded.

use super::{
    apply_patch, revert_patch, validate_patch, PatchApplyResponse, PatchRevertResponse,
    PatchValidateResponse,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Outcome of one propose-diff cycle. `Reverted` is the only full pass;
/// the rest are honest, non-panicking reports of where a diff fell down.
///
/// Several variant payloads (`errors`, `reason`) are carried purely for
/// the `{outcome:?}` diagnostic the smoke prints / panics with, so they
/// read as "never read" to dead-code analysis — hence the allow.
#[derive(Debug)]
#[allow(dead_code)]
enum CycleOutcome {
    /// Validated, applied, and the pre-apply checkpoint restored cleanly.
    Reverted { touched: Vec<String> },
    /// `validate_patch` rejected the diff (shape / path safety).
    Invalid { errors: Vec<String> },
    /// Validated but `apply_patch` failed (e.g. pre-image mismatch). The
    /// applier rolled back, so disk is unchanged.
    ApplyFailed { reason: String },
    /// Applied but the revert failed — the one case that DID leave the
    /// fixture mutated; surfaced loudly.
    RevertFailed { reason: String },
}

impl CycleOutcome {
    fn is_pass(&self) -> bool {
        matches!(self, CycleOutcome::Reverted { .. })
    }
}

/// Run the full validate → apply → revert cycle for `diff` against
/// `root` using Plume's real patch entry points. Pure orchestration: it
/// makes no assertions, so both the unit tests and the model-driven smoke
/// can interpret the outcome themselves.
fn run_propose_diff_cycle(root: &Path, diff: &str) -> CycleOutcome {
    match validate_patch(root, diff) {
        PatchValidateResponse::Err(e) => {
            return CycleOutcome::Invalid {
                errors: e.errors.into_iter().map(|err| err.message).collect(),
            };
        }
        PatchValidateResponse::Ok(_) => {}
    }

    let checkpoint = match apply_patch(root, diff) {
        PatchApplyResponse::Ok(ok) => ok.checkpoint,
        PatchApplyResponse::Err(e) => {
            return CycleOutcome::ApplyFailed {
                reason: format!("{e:?}"),
            };
        }
    };

    match revert_patch(root, &checkpoint) {
        PatchRevertResponse::Ok(ok) => CycleOutcome::Reverted {
            touched: ok.restored.into_iter().map(|f| f.path).collect(),
        },
        PatchRevertResponse::Err(e) => CycleOutcome::RevertFailed {
            reason: format!("{e:?}"),
        },
    }
}

// ─── In-container fixture helpers ────────────────────────────────────────

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-propose-diff-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Seed `greet.py` (the file the smoke prompt asks the model to edit) and
/// return the canonical fixture root + the seed content.
fn seed_fixture(td: &TempDir) -> (PathBuf, &'static str) {
    let root = fs::canonicalize(&td.path).expect("canonicalize fixture");
    let seed = "def greet(name):\n    return \"Hello, \" + name\n";
    fs::write(root.join("greet.py"), seed).expect("write seed");
    (root, seed)
}

// ─── In-container tests (no model) ───────────────────────────────────────

#[test]
fn cycle_applies_and_reverts_a_valid_diff() {
    let td = TempDir::new("valid");
    let (root, seed) = seed_fixture(&td);

    // A clean unified diff that matches the seed pre-image.
    let diff = "--- a/greet.py\n\
        +++ b/greet.py\n\
        @@ -1,2 +1,2 @@\n\
         def greet(name):\n\
        -    return \"Hello, \" + name\n\
        +    return f\"Hello, {name}!\"\n";

    let outcome = run_propose_diff_cycle(&root, diff);
    assert!(outcome.is_pass(), "expected a full cycle, got {outcome:?}");
    if let CycleOutcome::Reverted { touched, .. } = &outcome {
        assert!(touched.iter().any(|p| p == "greet.py"), "restored greet.py");
    }
    // Disk is back to the seed after revert.
    let after = fs::read_to_string(root.join("greet.py")).unwrap();
    assert_eq!(after, seed, "revert restored the seed content");
}

#[test]
fn cycle_reports_invalid_diff_and_leaves_disk_untouched() {
    let td = TempDir::new("invalid");
    let (root, seed) = seed_fixture(&td);

    // Path escape — validation must reject before any write.
    let diff = "--- a/../../etc/passwd\n\
        +++ b/../../etc/passwd\n\
        @@ -1,1 +1,1 @@\n\
        -x\n\
        +y\n";

    let outcome = run_propose_diff_cycle(&root, diff);
    match &outcome {
        CycleOutcome::Invalid { errors } => assert!(!errors.is_empty()),
        other => panic!("expected Invalid, got {other:?}"),
    }
    // The seeded file is untouched and no stray file appeared.
    assert_eq!(fs::read_to_string(root.join("greet.py")).unwrap(), seed);
}

#[test]
fn cycle_reports_apply_failure_on_preimage_mismatch_without_writing() {
    let td = TempDir::new("mismatch");
    let (root, seed) = seed_fixture(&td);

    // Well-formed diff, but its context does not match the seed on disk —
    // apply must fail (PreImageMismatch) and roll back.
    let diff = "--- a/greet.py\n\
        +++ b/greet.py\n\
        @@ -1,2 +1,2 @@\n\
         def greet(name):\n\
        -    return \"Goodbye, \" + name\n\
        +    return \"Hi, \" + name\n";

    let outcome = run_propose_diff_cycle(&root, diff);
    match &outcome {
        CycleOutcome::ApplyFailed { reason } => {
            assert!(!reason.is_empty(), "apply failure reason present");
        }
        other => panic!("expected ApplyFailed, got {other:?}"),
    }
    // Disk unchanged: the applier rolled back.
    assert_eq!(fs::read_to_string(root.join("greet.py")).unwrap(), seed);
}

// ─── Model-driven smoke (Mac only; ignored by default) ───────────────────

/// The entry point `scripts/smoke-qwen-propose-diff.sh` drives. It reads
/// two environment variables and runs the same cycle:
///
/// - `PLUME_SMOKE_FIXTURE` — the seeded fixture root the diff edits.
/// - `PLUME_SMOKE_DIFF_FILE` — a file holding the model's diff reply.
///
/// `#[ignore]` so the normal suite never needs a model. Invoke with:
/// `cargo test --bin plume -- --ignored --exact
/// patch::propose_diff_smoke_tests::qwen_propose_diff_smoke`.
#[test]
#[ignore = "needs a local model diff via scripts/smoke-qwen-propose-diff.sh"]
fn qwen_propose_diff_smoke() {
    let fixture = std::env::var("PLUME_SMOKE_FIXTURE")
        .expect("PLUME_SMOKE_FIXTURE must point at the seeded fixture root");
    let diff_file = std::env::var("PLUME_SMOKE_DIFF_FILE")
        .expect("PLUME_SMOKE_DIFF_FILE must point at the model's diff reply");
    let root = fs::canonicalize(&fixture).expect("fixture root must exist");
    let diff = fs::read_to_string(&diff_file).expect("diff file must be readable");

    let outcome = run_propose_diff_cycle(&root, &diff);
    println!("propose-diff cycle outcome: {outcome:?}");
    assert!(
        outcome.is_pass(),
        "local model diff did not survive the validate→apply→revert cycle: {outcome:?}"
    );
}
