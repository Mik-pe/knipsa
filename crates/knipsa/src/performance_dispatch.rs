//! Ordered standard-case dispatch for floating-point boolean operations.

#[path = "convex_dispatch.rs"]
mod convex;

use crate::{BooleanRequestD, PathsD};

pub(crate) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<Result<PathsD, ()>> {
    if let Some(result) = convex::try_boolean_opd(request) {
        return Some(Ok(result));
    }
    crate::standard_dispatch::try_boolean_opd(request)
}
