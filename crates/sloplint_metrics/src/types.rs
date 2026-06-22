//! Type-annotation coverage for a single function signature — how many annotatable parameters
//! carry a type hint, and whether the return is annotated. Measures *under*-annotation as a
//! readability/tooling concern (slop is badness, not provenance), never penalizing fully-typed code.

use crate::size::receiver_count;
use sloplint_python::ast::StmtFunctionDef;

/// Type-hint coverage for one function signature: `(typed_params, annotatable_params,
/// has_return_annotation)`.
///
/// *Annotatable* params are the positional and keyword params (positional-only + regular +
/// keyword-only). The `self`/`cls` receiver of a non-static method is excluded — it is
/// conventionally unannotated and not a quality signal — as are `*args`/`**kwargs`, which are
/// variadic collectors that are rarely annotated and would only dilute the ratio. A function with
/// no annotatable params yields `0/0`: it contributes nothing to coverage rather than being
/// penalized.
///
/// This measures *under*-annotation as a quality concern (missing types are harder to read and
/// refactor and weaken tooling). The "bad" direction is **low** coverage only — fully-typed code
/// is neutral-to-good and is never itself a slop signal (slop is badness, not provenance).
pub(crate) fn type_hint_coverage(function: &StmtFunctionDef) -> (usize, usize, bool) {
    let params = &function.parameters;
    // Drop exactly one leading positional for a non-static method whose first parameter is the
    // `self`/`cls` receiver (see [`receiver_count`]).
    let skip_receiver = receiver_count(function);

    let mut annotatable = 0usize;
    let mut typed = 0usize;
    for param in params
        .posonlyargs
        .iter()
        .chain(&params.args)
        .chain(&params.kwonlyargs)
        .skip(skip_receiver)
    {
        annotatable += 1;
        if param.parameter.annotation.is_some() {
            typed += 1;
        }
    }
    (typed, annotatable, function.returns.is_some())
}
