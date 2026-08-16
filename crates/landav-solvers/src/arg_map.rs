//! [`ArgMap`] - the positional map between `Arg_i` and a named variable.

use landav_bound::{Bound, Symbol, VarId};
use landav_its::Its;

use crate::{MAX_ARGS, solver::Solver, solver_error::SolverError};

/// The variable tuple a solver's answer is expressed in, in order.
///
/// # The single most dangerous mapping in this crate
///
/// `landav-its` emits `(VAR vi vn)` and KoAT answers about `Arg_0` and
/// `Arg_1`. The correspondence is positional and there is nothing in the
/// answer to check it against: `Arg_1+2` is a perfectly well-formed bound
/// whichever variable `Arg_1` turns out to mean. Getting it wrong does not
/// produce an error, a warning, or an implausible-looking number. It produces
/// a **wrong answer that looks right** - a bound attributed to the loop
/// counter instead of to the parameter, or to `m` instead of `n`.
///
/// So the mapping is pinned in three independent places, and it needs all
/// three:
///
/// 1. **Here.** The order is [`Its::vars`], which is the same order
///    [`landav_its::koat::render`] writes into `(VAR ...)` and into every
///    rule. An index past the end is [`SolverError::ArgIndexOutOfRange`],
///    never a wrap, a clamp or a fresh name.
/// 2. **In the argument vector.** KoAT's default preprocessing includes
///    `eliminate`, which drops variables that "do not contribute to the
///    problem" *and renumbers the survivors*. A system declaring
///    `(VAR vaaa vi vn)` whose `vaaa` is never read then answers about `Arg_1`
///    where this crate expects `Arg_2`. [`crate::Solver::argv`] therefore
///    passes a preprocessor list with `eliminate` removed, and
///    `tests/koat_answers.rs` pins that list.
/// 3. **Against a live solver.** `tests/invocation.rs` builds a system with a
///    dead leading parameter and asserts the bound names the parameter rather
///    than the counter. That test is the only thing that can notice if the
///    flag ever stops working, and it says so out loud when `koat2` is absent.
///
/// # Parameters, and bounds that cannot be evaluated
///
/// [`Its::params`] is the subset a caller can actually supply values for. A
/// bound over one of the lowering's fresh loop counters is *sound* - it is a
/// true statement about the transition system - but it is not something a
/// caller can evaluate, because the counter's initial value is not an input.
/// [`ArgMap::unevaluable`] finds those, and [`crate::Report`] turns them into
/// blame rather than into a proved bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgMap {
    /// Every variable of the system, in the order the solver sees them.
    vars: Vec<Symbol>,
    /// The subset a derived bound may be expressed in, sorted.
    params: Vec<Symbol>,
}

impl ArgMap {
    /// The map for `its`, taken from [`Its::vars`] and [`Its::params`].
    ///
    /// # Errors
    ///
    /// [`SolverError::TooManyVariables`] if the system declares more than
    /// [`MAX_ARGS`] variables.
    pub fn for_its(its: &Its) -> Result<Self, SolverError> {
        Self::new(
            its.vars().iter().map(|v| v.symbol().clone()).collect(),
            its.params().iter().map(|v| v.symbol().clone()).collect(),
        )
    }

    /// A map over `vars`, of which `params` are the evaluable ones.
    ///
    /// # Errors
    ///
    /// [`SolverError::TooManyVariables`] if `vars` is longer than
    /// [`MAX_ARGS`]. The cap is what keeps the `u32` index this crate reads
    /// out of solver output meaningful: an unbounded declaration list would
    /// make every index in range and so make the out-of-range check vacuous.
    /// One type parameter rather than two, deliberately: it lets an empty
    /// parameter list be written as a bare `Vec::new()` at a call site that
    /// has already fixed the name type, which is how the frozen-invariant
    /// suite spells "a system with no evaluable variables".
    pub fn new<S: Into<Symbol>>(vars: Vec<S>, params: Vec<S>) -> Result<Self, SolverError> {
        if vars.len() > MAX_ARGS {
            return Err(SolverError::TooManyVariables {
                got: vars.len(),
                limit: MAX_ARGS,
            });
        }
        let mut params: Vec<Symbol> = params.into_iter().map(Into::into).collect();
        params.sort();
        params.dedup();
        Ok(Self {
            vars: vars.into_iter().map(Into::into).collect(),
            params,
        })
    }

    /// The empty map: no variables, so every index is out of range.
    ///
    /// Present so that a caller which cannot build a real map has something
    /// total to fall back to. It refuses every `Arg_i`, which is the safe
    /// direction.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            vars: Vec::new(),
            params: Vec::new(),
        }
    }

    /// How many variables the system declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// Whether the system declares no variables at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The variable at position `index`.
    ///
    /// # Errors
    ///
    /// [`SolverError::ArgIndexOutOfRange`] if the system declares no variable
    /// at that position. There is no fallback: a synthesised name would be a
    /// bound over a variable the program does not have, and a clamp would
    /// attribute the bound to the wrong one.
    pub fn name(&self, index: u32) -> Result<&Symbol, SolverError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.vars.get(index))
            .ok_or(SolverError::ArgIndexOutOfRange {
                solver: Solver::Koat,
                index,
                declared: self.vars.len(),
            })
    }

    /// Whether `name` is a variable a caller can supply a value for.
    #[must_use]
    pub fn is_param(&self, name: &Symbol) -> bool {
        self.params.binary_search(name).is_ok()
    }

    /// The variables of `bound` that are not parameters, in canonical order.
    ///
    /// Empty for a bound a caller can evaluate. Non-empty for one that is
    /// sound but expressed in something the caller does not control, which is
    /// a partial answer rather than a proved one.
    #[must_use]
    pub fn unevaluable(&self, bound: &Bound) -> Vec<VarId> {
        bound
            .vars()
            .into_iter()
            .filter(|var| !self.is_param(var.symbol()))
            .collect()
    }
}
