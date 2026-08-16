//! [`Report`] - one solver's answer about one function, ready to publish.

use landav_bound::{Assumption, Blame, Blames, Bound, Lifted, Origin, Symbol, VarId, Verdict};

use crate::{
    answer::Answer, arg_map::ArgMap, direction::Direction, solver::Solver,
    solver_error::SolverError,
};

/// What one solver said about one function, with the blame that goes with it.
///
/// # Blame is computed here, not deferred
///
/// [`landav_bound::Verdict::classify`] refuses to publish an `omega`-bearing
/// bound with an empty ledger - that is `BoundError::UnblamedOmega`, a tool
/// error rather than a clean report. So the ledger has to exist by the time an
/// answer becomes a verdict, and the only place that knows *why* it is
/// `omega` is here, next to the answer that was read.
///
/// Three things earn blame, and they are different facts:
///
/// * **the solver found no bound** - `inf {Infinity}`, the common case.
///   [`Assumption::TerminationNotProved`], naming the function. KoAT returning
///   `inf` almost always means it could not rank a loop.
/// * **the bound mentions something the caller cannot supply** - one of the
///   lowering's fresh loop counters, say. The bound is *sound*; it is just not
///   evaluable, so it is a partial answer rather than a proved one, with
///   [`Assumption::SizeNotBounded`] naming each such variable.
/// * **the report is a lower bound** - a statement about how *slow* the
///   program can be, which is not an upper bound and never becomes one. Its
///   published bound is `omega`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    solver: Solver,
    answer: Answer,
    raw: String,
    function: Symbol,
    origin: Origin,
    blames: Option<Blames>,
}

impl Report {
    /// Pair `answer` with the function it is about, computing its blame.
    #[must_use]
    pub fn new(
        solver: Solver,
        answer: Answer,
        raw: impl Into<String>,
        function: impl Into<Symbol>,
        origin: Origin,
        map: &ArgMap,
    ) -> Self {
        let function = function.into();
        let blames = ledger(solver, &answer, &function, &origin, map);
        Self {
            solver,
            answer,
            raw: raw.into(),
            function,
            origin,
            blames,
        }
    }

    /// Which solver answered.
    #[must_use]
    pub const fn solver(&self) -> Solver {
        self.solver
    }

    /// Which side of the runtime this answer bounds.
    #[must_use]
    pub const fn direction(&self) -> Direction {
        self.solver.direction()
    }

    /// What it said, once read.
    #[must_use]
    pub const fn answer(&self) -> &Answer {
        &self.answer
    }

    /// What it printed, verbatim.
    ///
    /// Kept so that a failure downstream can quote the solver rather than
    /// paraphrase it.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The function this answer is about.
    #[must_use]
    pub const fn function(&self) -> &Symbol {
        &self.function
    }

    /// Where that function was.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// What was left unaccounted for, if anything.
    #[must_use]
    pub const fn blames(&self) -> Option<&Blames> {
        self.blames.as_ref()
    }

    /// This answer as a publishable verdict.
    ///
    /// [`Verdict::Proved`] only for a finite upper bound over variables the
    /// caller can supply; [`Verdict::Partial`] for everything else, always
    /// with a ledger naming the function.
    ///
    /// # Errors
    ///
    /// [`SolverError::Unpublishable`] if the bound algebra refuses the pair,
    /// which means an `omega` reached it without blame - a bug in this crate
    /// rather than a property of the solver's output.
    pub fn verdict(&self) -> Result<Verdict, SolverError> {
        let bound = match (self.direction(), &self.answer) {
            (Direction::Upper, Answer::Symbolic { bound, .. }) => bound.clone(),
            // A lower bound says how slow the program can be. It is not an
            // upper bound, and nothing turns it into one.
            _ => Bound::omega(),
        };
        Verdict::classify(
            Lifted::Elem(bound),
            self.origin.clone(),
            self.blames.clone(),
        )
        .map_err(|error| SolverError::Unpublishable {
            detail: error.to_string(),
        })
    }
}

/// The blame ledger an answer carries, or `None` when there is nothing
/// unaccounted for.
fn ledger(
    solver: Solver,
    answer: &Answer,
    function: &Symbol,
    origin: &Origin,
    map: &ArgMap,
) -> Option<Blames> {
    let blame = |assumption: Assumption| Blame {
        unaccounted: function.clone(),
        assumption,
        origin: origin.clone(),
    };

    if solver.direction() == Direction::Lower {
        return Some(Blames::new(blame(Assumption::ResourceNotModelled {
            detail: Symbol::from("a lower bound is not an upper bound"),
        })));
    }

    match answer {
        Answer::Unknown => Some(Blames::new(blame(Assumption::TerminationNotProved))),
        Answer::Class(_) => Some(Blames::new(blame(Assumption::ResourceNotModelled {
            detail: Symbol::from("the solver stated a growth class and no bound"),
        }))),
        Answer::Symbolic { bound, .. } => {
            let unevaluable = map.unevaluable(bound);
            let mut records = unevaluable.into_iter().map(|var: VarId| Blame {
                unaccounted: var.symbol().clone(),
                assumption: Assumption::SizeNotBounded { var },
                origin: origin.clone(),
            });
            let first = records.next()?;
            let mut blames = Blames::new(first);
            for record in records {
                blames.insert(record);
            }
            Some(blames)
        }
    }
}
