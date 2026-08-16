//! [`Construct`] - the named vocabulary of things this lowering will not do.

/// What a refusal is *about*.
///
/// # This enum is the diagnostic vocabulary
///
/// Non-negotiable 3 says failure must carry blame, and `LAN-67` criterion 4
/// says an unsupported construct must produce an explicit diagnostic rather
/// than silent truncation. A bare "unsupported" satisfies neither: it names
/// nothing, so it cannot be counted, grouped, or acted on. Every variant here
/// is a *name* that a coverage report can aggregate by, which is exactly what
/// `LAN-68` criterion 2 needs and the reason this type is public and
/// exhaustively documented rather than a string.
///
/// **There is deliberately no `Other` or `Unknown` variant**, for the same
/// reason [`landav_bound::Assumption`] has none: adding one must be a
/// reviewable diff against the blame rule, not the path of least resistance.
/// A frontend meeting something genuinely new picks the closest variant and
/// puts the specifics in the `detail` [`landav_bound::Symbol`] of the
/// [`crate::Unsupported`] record.
///
/// # Frontend-emitted versus lowering-raised
///
/// Most variants are emitted by a *frontend*, which is the only layer that
/// knows what a comprehension or a method call is. Three are raised by the
/// lowering itself, because they are properties of the arithmetic rather than
/// of the source language: [`Construct::ArithmeticOverflow`],
/// [`Construct::PolynomialDegree`] and [`Construct::PolynomialSize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Construct {
    /// A value the frontend could not establish is an integer.
    ///
    /// The catch-all for the *type* question, and the one a frontend reaches
    /// for when an annotation is missing. Lowering a non-integer as though it
    /// were an integer is the single fastest route to an unsound bound, so an
    /// unproven type is a refusal rather than an assumption.
    NonIntegerValue,
    /// A call to anything: a function, a method, a constructor.
    ///
    /// Refused rather than approximated because a call has an unknown *effect*
    /// as well as an unknown value, and there is no sound over-approximation
    /// of an unknown effect on the integer state.
    Call,
    /// An attribute access, and with it the object model.
    Attribute,
    /// An index or a slice, and with it the container model.
    Subscript,
    /// A list, tuple, set, dictionary or string value.
    Collection,
    /// A comprehension or generator expression.
    Comprehension,
    /// `try`, `except`, `finally`, `raise`, or `with`: non-local control flow.
    ExceptionalControlFlow,
    /// `break` or `continue`.
    ///
    /// Sound to support and merely not yet supported - see the crate-level
    /// "What is refused" table, which records this as the first construct to
    /// add.
    LoopJump,
    /// A loop over anything that is not an integer range with a literal step.
    ///
    /// Covers iteration over a container, over an iterator, and over
    /// `range(a, b, s)` where the sign of `s` is not known at lowering time -
    /// the sign decides which way the loop guard points, so an unknown sign
    /// is an unknown guard.
    UnboundedIteration,
    /// Floor division, modulo, or true division.
    ///
    /// Exactly encodable as a guarded nondeterministic update and refused only
    /// because that needs machinery this story does not build; see the
    /// crate-level docs.
    IntegerDivision,
    /// Exponentiation whose exponent is not a small non-negative literal.
    NonPolynomialPower,
    /// A bitwise or shift operator.
    BitwiseOperator,
    /// A function definition, class definition, import, or other declaration
    /// nested inside the fragment being lowered.
    Declaration,
    /// `global`, `nonlocal`, `del`, or any other binding-form statement that
    /// changes what a name means.
    BindingForm,
    /// An assignment whose target is not a single plain variable: tuple
    /// unpacking, starred targets, augmented targets that are not names.
    ComplexAssignmentTarget,
    /// `yield`, `await`, or an `async` construct.
    Coroutine,
    /// A structural pattern match.
    PatternMatch,
    /// A value produced by a conditional or boolean expression rather than by
    /// arithmetic: `a if c else b`, a comparison used as a number, `and`/`or`
    /// in a value position.
    ///
    /// Sound to support and genuinely useful — a conditional expression is two
    /// guarded transitions, exactly like the statement form — but it needs the
    /// expression translation to be able to emit *statements*, which restructures
    /// the whole frontend traversal. Refused rather than approximated.
    ConditionalExpression,
    /// An integer literal or intermediate coefficient outside the range the
    /// lowering represents.
    ///
    /// Raised by the lowering. Wrapping would turn a large constant into a
    /// small one and silently change the program's meaning, which is the
    /// truncation criterion 4 forbids, so the coefficient arithmetic is
    /// checked and overflow refuses.
    ArithmeticOverflow,
    /// A polynomial whose degree exceeds [`crate::MAX_DEGREE`].
    ///
    /// Raised by the lowering.
    PolynomialDegree,
    /// A polynomial with more monomials than [`crate::MAX_MONOMIALS`].
    ///
    /// Raised by the lowering. `(a + b + c)^8` is not deep, not long, and has
    /// 45 terms; a cap on depth alone does not bound the work.
    PolynomialSize,
}

impl Construct {
    /// A short, stable, machine-readable tag.
    ///
    /// Written out rather than derived from the variant name, so that renaming
    /// a Rust identifier - a pure readability refactor that breaks no build -
    /// cannot change a diagnostic code that a report has grouped by or a
    /// baseline has pinned.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::NonIntegerValue => "non-integer-value",
            Self::Call => "call",
            Self::Attribute => "attribute",
            Self::Subscript => "subscript",
            Self::Collection => "collection",
            Self::Comprehension => "comprehension",
            Self::ExceptionalControlFlow => "exceptional-control-flow",
            Self::LoopJump => "loop-jump",
            Self::UnboundedIteration => "unbounded-iteration",
            Self::IntegerDivision => "integer-division",
            Self::NonPolynomialPower => "non-polynomial-power",
            Self::BitwiseOperator => "bitwise-operator",
            Self::Declaration => "declaration",
            Self::BindingForm => "binding-form",
            Self::ComplexAssignmentTarget => "complex-assignment-target",
            Self::Coroutine => "coroutine",
            Self::PatternMatch => "pattern-match",
            Self::ConditionalExpression => "conditional-expression",
            Self::ArithmeticOverflow => "arithmetic-overflow",
            Self::PolynomialDegree => "polynomial-degree",
            Self::PolynomialSize => "polynomial-size",
        }
    }

    /// One sentence naming what was refused, for a human reading a report.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::NonIntegerValue => "value is not a proven integer",
            Self::Call => "call to a function or method",
            Self::Attribute => "attribute access",
            Self::Subscript => "index or slice",
            Self::Collection => "list, tuple, set, dict or string value",
            Self::Comprehension => "comprehension or generator expression",
            Self::ExceptionalControlFlow => "exception handling or context manager",
            Self::LoopJump => "break or continue",
            Self::UnboundedIteration => "loop over something other than an integer range",
            Self::IntegerDivision => "division or modulo",
            Self::NonPolynomialPower => "exponent is not a small non-negative literal",
            Self::BitwiseOperator => "bitwise or shift operator",
            Self::Declaration => "nested definition or import",
            Self::BindingForm => "global, nonlocal or del",
            Self::ComplexAssignmentTarget => "assignment target is not a plain variable",
            Self::Coroutine => "yield, await or async construct",
            Self::PatternMatch => "structural pattern match",
            Self::ConditionalExpression => "conditional or boolean expression used as a value",
            Self::ArithmeticOverflow => "integer constant too large to represent",
            Self::PolynomialDegree => "polynomial degree limit exceeded",
            Self::PolynomialSize => "polynomial term-count limit exceeded",
        }
    }

    /// Every variant, in declaration order.
    ///
    /// Published so that `LAN-68`'s coverage report can enumerate the whole
    /// vocabulary - including the constructs that were *not* hit, which is the
    /// half of a coverage report that carries information.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::NonIntegerValue,
            Self::Call,
            Self::Attribute,
            Self::Subscript,
            Self::Collection,
            Self::Comprehension,
            Self::ExceptionalControlFlow,
            Self::LoopJump,
            Self::UnboundedIteration,
            Self::IntegerDivision,
            Self::NonPolynomialPower,
            Self::BitwiseOperator,
            Self::Declaration,
            Self::BindingForm,
            Self::ComplexAssignmentTarget,
            Self::Coroutine,
            Self::PatternMatch,
            Self::ConditionalExpression,
            Self::ArithmeticOverflow,
            Self::PolynomialDegree,
            Self::PolynomialSize,
        ]
    }
}

impl core::fmt::Display for Construct {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.tag())
    }
}
