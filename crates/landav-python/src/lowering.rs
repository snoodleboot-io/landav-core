//! Translating typed Python into the language-neutral numeric fragment.
//!
//! # This is the only file that knows both languages
//!
//! Non-negotiable 4 says no Python assumption may live outside this crate, and
//! the crate graph enforces the direction: `landav-python` depends on
//! `landav-its`, never the reverse. Everything Python-specific about the
//! lowering therefore lives here — that `range` is a builtin with one, two or
//! three arguments; that a bare integer in a condition means `!= 0`; that
//! `x += 1` means `x = x + 1`; that a string constant at the top of a body is
//! a docstring and not a value. [`landav_its`] knows none of it, and receives
//! a [`landav_its::SourceProgram`] built out of integers, arithmetic and three control
//! constructs.
//!
//! # "Typed" means proved, not annotated
//!
//! The story is *typed* Python, and the fragment's contract is that every
//! variable denotes a mathematical integer. Python will not tell us that, so
//! `integer_names` below works it out: parameters annotated `int` are the seed, and
//! a local joins them only if **every** assignment to it has an integer
//! right-hand side. That is a least-fixed-point computation, started
//! optimistically and refined until it stops shrinking.
//!
//! A name that does not survive is not guessed at — every read of it becomes
//! [`Construct::NonIntegerValue`], naming the variable. Guessing here is the
//! shortest path to an unsound bound: a `float` treated as an integer makes
//! every guard mentioning it mean something else.
//!
//! # Refusal is the default, not the exception
//!
//! Every `match` over a Python node ends in an arm that produces an
//! `Unsupported` node. That inversion is the whole of `LAN-67` criterion 4: in
//! a translator whose fallback skips what it does not recognise, silence is
//! free and a diagnostic must be remembered; here, *not thinking about a
//! construct* produces a loud refusal rather than a quiet unsound bound. There
//! is no arm anywhere below that drops a node on the floor.
//!
//! # Traversal
//!
//! Expressions and conditions are translated through an explicit worklist,
//! because their depth is bounded only by `MAX_EXPRESSION_DEPTH` (10 000) and a
//! recursive translation of a chain that long risks the stack — an abort, not
//! an error. Statement bodies are translated recursively, which is safe because
//! the byte-level guard in this crate's `syntax` module rejects block nesting past
//! `MAX_NESTING_DEPTH` (120) before the parser ever runs.

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

use landav_bound::Origin;
use landav_its::{
    ArithOp, CompareOp, CondId, Construct, ExprId, RangeSpec, SourceProgramBuilder, StmtId, VarName,
};
use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt};

use crate::{
    analysis::parse_guarded, location::Location, lowered_function::LoweredFunction,
    python_error::PythonError, syntax::LineIndex,
};

/// The largest exponent a `**` may carry and stay in the fragment.
///
/// Matches [`landav_its::MAX_DEGREE`]: beyond it the lowering would refuse
/// anyway, and refusing here names the construct rather than the polynomial.
const MAX_EXPONENT: u32 = landav_its::MAX_DEGREE;

/// Translates every top-level function in `source` into the numeric fragment.
///
/// `path` is not read; it is the label stamped into every position, exactly as
/// in [`crate::analyze_module`].
///
/// A function is returned for **every** top-level `def`, whether or not it is
/// inside the fragment. Deciding that is [`landav_its::lower`]'s job, and it
/// answers with a named refusal per construct; returning only the functions
/// that happen to lower would throw that answer away and make coverage
/// unmeasurable, which is precisely what `LAN-68` needs.
///
/// # Errors
///
/// [`PythonError::Parse`] if `source` is not valid Python 3, or is nested more
/// deeply than the frontend will parse. Nothing else: a construct outside the
/// fragment is not an error here, it is an `Unsupported` node in the program.
pub fn lower_module(path: &Path, source: &str) -> Result<Vec<LoweredFunction>, PythonError> {
    let (module, index) = parse_guarded(path, source)?;
    let mut lowered = Vec::new();
    for statement in &module {
        if let Stmt::FunctionDef(function) = statement {
            lowered.push(lower_function(path, &index, function));
        }
    }
    Ok(lowered)
}

/// Translates one `def`.
fn lower_function(
    path: &Path,
    index: &LineIndex,
    function: &ast::StmtFunctionDef,
) -> LoweredFunction {
    let integers = integer_names(function);
    let name = function.name.to_string();
    let location = position(path, index, function);
    let origin = origin_of(path, index, function);

    let params: Vec<VarName> = integer_parameters(function)
        .into_iter()
        .map(VarName::new)
        .collect();

    let mut translator = Translator {
        path,
        index,
        builder: SourceProgramBuilder::new(name.clone(), origin, params),
        integers,
    };
    let body = translator.block(&function.body);
    let program = translator.builder.build(body);

    LoweredFunction::new(name, location, program)
}

// ---------------------------------------------------------------------------
// which names are integers
// ---------------------------------------------------------------------------

/// The parameters annotated `int`, in declaration order.
fn integer_parameters(function: &ast::StmtFunctionDef) -> Vec<String> {
    let arguments = &function.args;
    arguments
        .posonlyargs
        .iter()
        .chain(arguments.args.iter())
        .filter(|parameter| annotation_is_int(parameter.def.annotation.as_deref()))
        .map(|parameter| parameter.def.arg.to_string())
        .collect()
}

/// Whether an annotation says `int`.
///
/// Only the bare name. `Optional[int]` admits `None`, `bool` is a subtype whose
/// arithmetic agrees but whose `Optional` does not, and a string annotation is
/// a forward reference this crate does not resolve. Each of those could be
/// accepted later; none may be assumed now.
fn annotation_is_int(annotation: Option<&Expr>) -> bool {
    matches!(annotation, Some(Expr::Name(name)) if name.id.as_str() == "int")
}

/// The names that provably hold integers throughout `function`.
///
/// A least fixed point: start with every assigned name plus the `int`
/// parameters, then repeatedly drop any name with an assignment whose
/// right-hand side is not an integer under the current set. Shrinking only, so
/// it terminates in at most as many rounds as there are names.
fn integer_names(function: &ast::StmtFunctionDef) -> BTreeSet<String> {
    let arguments = &function.args;
    // A parameter without an `int` annotation can never be an integer,
    // whatever the body later assigns to it: it holds whatever the caller
    // passed on entry, and that is the value a loop guard would read.
    let unannotated: BTreeSet<String> = arguments
        .posonlyargs
        .iter()
        .chain(arguments.args.iter())
        .chain(arguments.kwonlyargs.iter())
        .filter(|parameter| !annotation_is_int(parameter.def.annotation.as_deref()))
        .map(|parameter| parameter.def.arg.to_string())
        .collect();

    let mut candidates: BTreeSet<String> = integer_parameters(function).into_iter().collect();
    for statement in crate::syntax::stmt_tree(&function.body) {
        for name in assigned_names(statement) {
            if !unannotated.contains(&name) {
                candidates.insert(name);
            }
        }
    }

    // Bounded by the number of candidates: each round either removes at least
    // one or stops.
    for _ in 0..=candidates.len() {
        let mut doomed: BTreeSet<String> = BTreeSet::new();
        for statement in crate::syntax::stmt_tree(&function.body) {
            collect_non_integer_bindings(statement, &candidates, &mut doomed);
        }
        if doomed.is_empty() {
            break;
        }
        for name in doomed {
            candidates.remove(&name);
        }
    }
    candidates
}

/// Every name this statement binds.
fn assigned_names(statement: &Stmt) -> Vec<String> {
    let mut names = Vec::new();
    match statement {
        Stmt::Assign(assign) => {
            for target in &assign.targets {
                crate::syntax::target_names(target, &mut names);
            }
        }
        Stmt::AugAssign(assign) => crate::syntax::target_names(&assign.target, &mut names),
        Stmt::AnnAssign(assign) => crate::syntax::target_names(&assign.target, &mut names),
        Stmt::For(loop_stmt) => crate::syntax::target_names(&loop_stmt.target, &mut names),
        _ => {}
    }
    names
}

/// Adds any name this statement binds to a non-integer value.
fn collect_non_integer_bindings(
    statement: &Stmt,
    candidates: &BTreeSet<String>,
    doomed: &mut BTreeSet<String>,
) {
    let mut condemn = |target: &Expr| {
        let mut names = Vec::new();
        crate::syntax::target_names(target, &mut names);
        for name in names {
            if candidates.contains(&name) {
                doomed.insert(name);
            }
        }
    };

    match statement {
        Stmt::Assign(assign) => {
            if !is_integer_expr(&assign.value, candidates) {
                for target in &assign.targets {
                    condemn(target);
                }
            }
            // Tuple unpacking binds names to elements this pass cannot see
            // into, so it never establishes integrality.
            for target in &assign.targets {
                if !matches!(target, Expr::Name(_)) {
                    condemn(target);
                }
            }
        }
        Stmt::AugAssign(assign) => {
            let arithmetic = arith_of(&assign.op).is_some();
            if !arithmetic || !is_integer_expr(&assign.value, candidates) {
                condemn(&assign.target);
            }
        }
        Stmt::AnnAssign(assign) => {
            if !annotation_is_int(Some(&assign.annotation)) {
                condemn(&assign.target);
            }
            if let Some(value) = &assign.value
                && !is_integer_expr(value, candidates)
            {
                condemn(&assign.target);
            }
        }
        Stmt::For(loop_stmt)
            if range_arguments(&loop_stmt.iter)
                .is_none_or(|args| !args.iter().all(|arg| is_integer_expr(arg, candidates))) =>
        {
            condemn(&loop_stmt.target);
        }
        _ => {}
    }
}

/// Whether an expression is certainly an integer under `candidates`.
///
/// Conservative in the safe direction throughout: an expression this cannot
/// prove integral is treated as non-integral, which costs coverage and never
/// soundness.
fn is_integer_expr(expr: &Expr, candidates: &BTreeSet<String>) -> bool {
    match expr {
        Expr::Constant(constant) => {
            matches!(constant.value, Constant::Int(_) | Constant::Bool(_))
        }
        Expr::Name(name) => candidates.contains(name.id.as_str()),
        Expr::BinOp(binary) => match binary.op {
            ast::Operator::Add | ast::Operator::Sub | ast::Operator::Mult => {
                is_integer_expr(&binary.left, candidates)
                    && is_integer_expr(&binary.right, candidates)
            }
            ast::Operator::Pow => {
                literal_exponent(&binary.right).is_some()
                    && is_integer_expr(&binary.left, candidates)
            }
            _ => false,
        },
        Expr::UnaryOp(unary) => {
            matches!(unary.op, ast::UnaryOp::USub | ast::UnaryOp::UAdd)
                && is_integer_expr(&unary.operand, candidates)
        }
        _ => false,
    }
}

/// The arguments of a `range(...)` call, or `None` if this is not one.
fn range_arguments(expr: &Expr) -> Option<&[Expr]> {
    let call = match expr {
        Expr::Call(call) => call,
        _ => return None,
    };
    let named = match call.func.as_ref() {
        Expr::Name(name) => name.id.as_str(),
        _ => return None,
    };
    if named != "range" || !call.keywords.is_empty() {
        return None;
    }
    match call.args.len() {
        1..=3 => Some(&call.args),
        _ => None,
    }
}

/// A `**` exponent that keeps the expression polynomial.
fn literal_exponent(expr: &Expr) -> Option<u32> {
    let Expr::Constant(constant) = expr else {
        return None;
    };
    let Constant::Int(value) = &constant.value else {
        return None;
    };
    let exponent = u32::try_from(value.clone()).ok()?;
    (exponent <= MAX_EXPONENT).then_some(exponent)
}

/// The fragment's operator for a Python one, if it has one.
const fn arith_of(op: &ast::Operator) -> Option<ArithOp> {
    match op {
        ast::Operator::Add => Some(ArithOp::Add),
        ast::Operator::Sub => Some(ArithOp::Sub),
        ast::Operator::Mult => Some(ArithOp::Mul),
        _ => None,
    }
}

/// Why a Python operator is not in the fragment.
const fn refusal_for(op: &ast::Operator) -> Construct {
    match op {
        ast::Operator::Div | ast::Operator::FloorDiv | ast::Operator::Mod => {
            Construct::IntegerDivision
        }
        ast::Operator::Pow => Construct::NonPolynomialPower,
        ast::Operator::LShift
        | ast::Operator::RShift
        | ast::Operator::BitOr
        | ast::Operator::BitXor
        | ast::Operator::BitAnd => Construct::BitwiseOperator,
        ast::Operator::MatMult | ast::Operator::Add | ast::Operator::Sub | ast::Operator::Mult => {
            Construct::NonIntegerValue
        }
    }
}

// ---------------------------------------------------------------------------
// the translation
// ---------------------------------------------------------------------------

struct Translator<'a> {
    path: &'a Path,
    index: &'a LineIndex,
    builder: SourceProgramBuilder,
    integers: BTreeSet<String>,
}

impl Translator<'_> {
    fn origin<T: Ranged>(&self, node: &T) -> Origin {
        origin_of(self.path, self.index, node)
    }

    // -- statements ---------------------------------------------------------

    /// Translates a block.
    ///
    /// Recursive, which is safe: block nesting is capped at
    /// `MAX_NESTING_DEPTH` by the byte-level guard before the parser runs.
    fn block(&mut self, body: &[Stmt]) -> Vec<StmtId> {
        body.iter().flat_map(|stmt| self.statement(stmt)).collect()
    }

    fn statement(&mut self, statement: &Stmt) -> Vec<StmtId> {
        match statement {
            Stmt::Assign(assign) => self.assign(assign),
            Stmt::AugAssign(assign) => self.aug_assign(assign),
            Stmt::AnnAssign(assign) => self.ann_assign(assign),
            Stmt::If(branch) => {
                let cond = self.condition(&branch.test);
                let then_body = self.block(&branch.body);
                let else_body = self.block(&branch.orelse);
                let origin = self.origin(branch);
                vec![self.builder.if_else(cond, then_body, else_body, origin)]
            }
            Stmt::While(loop_stmt) => {
                if !loop_stmt.orelse.is_empty() {
                    return vec![self.refuse_stmt_detailed(
                        Construct::ExceptionalControlFlow,
                        "while ... else",
                        loop_stmt,
                    )];
                }
                let cond = self.condition(&loop_stmt.test);
                let body = self.block(&loop_stmt.body);
                let origin = self.origin(loop_stmt);
                vec![self.builder.while_loop(cond, body, origin)]
            }
            Stmt::For(loop_stmt) => self.for_loop(loop_stmt),
            Stmt::Return(ret) => {
                // The returned value contributes nothing to runtime, but it may
                // *contain* something that has to be refused, so it is
                // translated. The node has no parent; `landav_its::lower`
                // scans the arenas, so an unattached refusal is still reported.
                if let Some(value) = &ret.value {
                    let _ = self.expression(value);
                }
                let origin = self.origin(ret);
                vec![self.builder.return_stmt(origin)]
            }
            Stmt::Pass(_) => Vec::new(),
            Stmt::Expr(bare) => self.bare_expression(bare),

            Stmt::Break(node) => vec![self.refuse_stmt(Construct::LoopJump, node)],
            Stmt::Continue(node) => vec![self.refuse_stmt(Construct::LoopJump, node)],
            Stmt::Raise(node) => {
                vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)]
            }
            Stmt::Try(node) => vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)],
            Stmt::TryStar(node) => {
                vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)]
            }
            Stmt::Assert(node) => vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)],
            Stmt::With(node) => vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)],
            Stmt::AsyncWith(node) => vec![self.refuse_stmt(Construct::Coroutine, node)],
            Stmt::AsyncFor(node) => vec![self.refuse_stmt(Construct::Coroutine, node)],
            Stmt::AsyncFunctionDef(node) => vec![self.refuse_stmt(Construct::Coroutine, node)],
            Stmt::FunctionDef(node) => vec![self.refuse_stmt(Construct::Declaration, node)],
            Stmt::ClassDef(node) => vec![self.refuse_stmt(Construct::Declaration, node)],
            Stmt::Import(node) => vec![self.refuse_stmt(Construct::Declaration, node)],
            Stmt::ImportFrom(node) => vec![self.refuse_stmt(Construct::Declaration, node)],
            Stmt::Global(node) => vec![self.refuse_stmt(Construct::BindingForm, node)],
            Stmt::Nonlocal(node) => vec![self.refuse_stmt(Construct::BindingForm, node)],
            Stmt::Delete(node) => vec![self.refuse_stmt(Construct::BindingForm, node)],
            Stmt::Match(node) => vec![self.refuse_stmt(Construct::PatternMatch, node)],
            Stmt::TypeAlias(node) => vec![self.refuse_stmt(Construct::Declaration, node)],
        }
    }

    fn assign(&mut self, assign: &ast::StmtAssign) -> Vec<StmtId> {
        let [target] = assign.targets.as_slice() else {
            // `a = b = 0` binds two names; the fragment's assignment binds one.
            let _ = self.expression(&assign.value);
            return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, assign)];
        };
        let Expr::Name(name) = target else {
            let _ = self.expression(&assign.value);
            return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, assign)];
        };
        let value = self.expression(&assign.value);
        self.bind(name.id.as_str(), value, assign, target)
    }

    fn aug_assign(&mut self, assign: &ast::StmtAugAssign) -> Vec<StmtId> {
        let Expr::Name(name) = assign.target.as_ref() else {
            let _ = self.expression(&assign.value);
            return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, assign)];
        };
        let Some(op) = arith_of(&assign.op) else {
            return vec![self.refuse_stmt(refusal_for(&assign.op), assign)];
        };
        // `x += e` is `x = x + e`. The expansion is a Python fact, and it
        // stays on this side of the boundary.
        let origin = self.origin(assign);
        let read = self.read(name.id.as_str(), assign.target.as_ref());
        let value = self.expression(&assign.value);
        let combined = self.builder.arith(op, read, value, origin);
        self.bind(name.id.as_str(), combined, assign, assign.target.as_ref())
    }

    fn ann_assign(&mut self, assign: &ast::StmtAnnAssign) -> Vec<StmtId> {
        let Some(value) = &assign.value else {
            // `x: int` with no value binds nothing.
            return Vec::new();
        };
        let Expr::Name(name) = assign.target.as_ref() else {
            let _ = self.expression(value);
            return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, assign)];
        };
        if !annotation_is_int(Some(&assign.annotation)) {
            let _ = self.expression(value);
            return vec![self.refuse_stmt_detailed(
                Construct::NonIntegerValue,
                name.id.as_str(),
                assign,
            )];
        }
        let translated = self.expression(value);
        self.bind(name.id.as_str(), translated, assign, assign.target.as_ref())
    }

    /// Emits `name = value`, or refuses if `name` is not a proven integer.
    fn bind<T: Ranged>(
        &mut self,
        name: &str,
        value: ExprId,
        node: &T,
        target: &Expr,
    ) -> Vec<StmtId> {
        if !self.integers.contains(name) {
            // The variable's value after this statement is unknown, so every
            // later guard mentioning it would be wrong. Refuse, naming it.
            let origin = self.origin(target);
            return vec![self.builder.unsupported_stmt_detailed(
                Construct::NonIntegerValue,
                name,
                origin,
            )];
        }
        let origin = self.origin(node);
        vec![self.builder.assign(VarName::new(name), value, origin)]
    }

    fn for_loop(&mut self, loop_stmt: &ast::StmtFor) -> Vec<StmtId> {
        if !loop_stmt.orelse.is_empty() {
            return vec![self.refuse_stmt_detailed(
                Construct::ExceptionalControlFlow,
                "for ... else",
                loop_stmt,
            )];
        }
        let Expr::Name(target) = loop_stmt.target.as_ref() else {
            return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, loop_stmt)];
        };
        let Some(arguments) = range_arguments(&loop_stmt.iter) else {
            // Iteration over a container, a generator, `enumerate`, `zip`: all
            // need a size model this fragment does not have.
            return vec![self.refuse_stmt(Construct::UnboundedIteration, loop_stmt)];
        };
        if !self.integers.contains(target.id.as_str()) {
            return vec![self.refuse_stmt_detailed(
                Construct::NonIntegerValue,
                target.id.as_str(),
                loop_stmt,
            )];
        }

        let origin = self.origin(loop_stmt);
        let (start, stop, step) = match arguments {
            [stop] => {
                let zero = self.builder.int(0, origin.clone());
                (zero, self.expression(stop), 1_i64)
            }
            [start, stop] => (self.expression(start), self.expression(stop), 1_i64),
            [start, stop, step] => {
                let Some(literal) = literal_step(step) else {
                    // The sign of the step decides which way the guard points,
                    // so a step this cannot read is a guard it cannot write.
                    return vec![self.refuse_stmt_detailed(
                        Construct::UnboundedIteration,
                        "range step is not a non-zero literal",
                        loop_stmt,
                    )];
                };
                (self.expression(start), self.expression(stop), literal)
            }
            _ => return vec![self.refuse_stmt(Construct::UnboundedIteration, loop_stmt)],
        };

        let Some(stride) = core::num::NonZeroI64::new(step) else {
            return vec![self.refuse_stmt_detailed(
                Construct::UnboundedIteration,
                "range step is zero",
                loop_stmt,
            )];
        };

        let body = self.block(&loop_stmt.body);
        vec![self.builder.for_range(
            VarName::new(target.id.as_str()),
            RangeSpec::new(start, stop, stride),
            body,
            origin,
        )]
    }

    /// A statement that is just an expression.
    fn bare_expression(&mut self, bare: &ast::StmtExpr) -> Vec<StmtId> {
        // A string constant on its own is a docstring, not a value. Treating it
        // as one would refuse every documented function in the corpus.
        if let Expr::Constant(constant) = bare.value.as_ref()
            && matches!(constant.value, Constant::Str(_) | Constant::Ellipsis)
        {
            return Vec::new();
        }
        // Anything else is translated; if it is pure arithmetic it is a no-op,
        // and if it is not, the `Unsupported` node it produces is refused by
        // the arena scan even though nothing points at it.
        let _ = self.expression(&bare.value);
        Vec::new()
    }

    fn refuse_stmt<T: Ranged>(&mut self, construct: Construct, node: &T) -> StmtId {
        let origin = self.origin(node);
        self.builder.unsupported_stmt(construct, origin)
    }

    fn refuse_stmt_detailed<T: Ranged>(
        &mut self,
        construct: Construct,
        detail: &str,
        node: &T,
    ) -> StmtId {
        let origin = self.origin(node);
        self.builder
            .unsupported_stmt_detailed(construct, detail, origin)
    }

    // -- conditions ---------------------------------------------------------

    /// Translates a condition, worklist-driven.
    fn condition(&mut self, root: &Expr) -> CondId {
        let ordered = postorder(root, condition_children);
        let mut built: HashMap<usize, CondId> = HashMap::new();

        for node in ordered {
            let id = self.build_condition(node, &built);
            built.insert(std::ptr::from_ref(node) as usize, id);
        }

        built
            .get(&(std::ptr::from_ref(root) as usize))
            .copied()
            .unwrap_or_else(|| {
                let origin = self.origin(root);
                self.builder
                    .unsupported_cond(Construct::ConditionalExpression, origin)
            })
    }

    fn build_condition(&mut self, node: &Expr, built: &HashMap<usize, CondId>) -> CondId {
        let origin = self.origin(node);
        let recall = |expr: &Expr| built.get(&(std::ptr::from_ref(expr) as usize)).copied();

        match node {
            Expr::BoolOp(boolean) => {
                let mut combined: Option<CondId> = None;
                for value in &boolean.values {
                    let Some(operand) = recall(value) else {
                        continue;
                    };
                    combined = Some(match combined {
                        None => operand,
                        Some(previous) => match boolean.op {
                            ast::BoolOp::And => self.builder.and(previous, operand, origin.clone()),
                            ast::BoolOp::Or => self.builder.or(previous, operand, origin.clone()),
                        },
                    });
                }
                combined.unwrap_or_else(|| {
                    self.builder
                        .unsupported_cond(Construct::ConditionalExpression, origin)
                })
            }

            Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => {
                match recall(&unary.operand) {
                    Some(operand) => self.builder.not(operand, origin),
                    None => self
                        .builder
                        .unsupported_cond(Construct::ConditionalExpression, origin),
                }
            }

            Expr::Compare(comparison) => self.comparison(comparison),

            // Everything else in a condition position is a truth test on a
            // value. In Python that means "not equal to zero" -- a language
            // fact, spelled out here so that Core never learns it. If the value
            // is not an integer the translation of it refuses, and the
            // comparison simply carries that refusal.
            other => {
                let value = self.expression(other);
                let zero = self.builder.int(0, origin.clone());
                self.builder.compare(CompareOp::Ne, value, zero, origin)
            }
        }
    }

    fn comparison(&mut self, comparison: &ast::ExprCompare) -> CondId {
        let origin = self.origin(comparison);
        let mut left = self.expression(&comparison.left);
        let mut combined: Option<CondId> = None;

        for (op, right_expr) in comparison.ops.iter().zip(comparison.comparators.iter()) {
            let right = self.expression(right_expr);
            let Some(operator) = compare_of(op) else {
                // `in`, `not in`, `is`, `is not`: membership and identity, both
                // of which need a model this fragment does not have.
                let construct = match op {
                    ast::CmpOp::In | ast::CmpOp::NotIn => Construct::Collection,
                    _ => Construct::NonIntegerValue,
                };
                return self.builder.unsupported_cond_detailed(
                    construct,
                    "membership or identity comparison",
                    origin,
                );
            };
            // `a < b < c` is `a < b and b < c`, with `b` evaluated once. Every
            // expression here is pure, so writing it twice changes nothing.
            let link = self.builder.compare(operator, left, right, origin.clone());
            combined = Some(match combined {
                None => link,
                Some(previous) => self.builder.and(previous, link, origin.clone()),
            });
            left = right;
        }

        combined.unwrap_or_else(|| {
            self.builder
                .unsupported_cond(Construct::ConditionalExpression, origin)
        })
    }

    // -- expressions --------------------------------------------------------

    /// Translates an expression, worklist-driven.
    fn expression(&mut self, root: &Expr) -> ExprId {
        let ordered = postorder(root, expression_children);
        let mut built: HashMap<usize, ExprId> = HashMap::new();

        for node in ordered {
            let id = self.build_expression(node, &built);
            built.insert(std::ptr::from_ref(node) as usize, id);
        }

        built
            .get(&(std::ptr::from_ref(root) as usize))
            .copied()
            .unwrap_or_else(|| {
                let origin = self.origin(root);
                self.builder
                    .unsupported_expr(Construct::NonIntegerValue, origin)
            })
    }

    fn build_expression(&mut self, node: &Expr, built: &HashMap<usize, ExprId>) -> ExprId {
        let origin = self.origin(node);
        let recall = |expr: &Expr| built.get(&(std::ptr::from_ref(expr) as usize)).copied();

        match node {
            Expr::Constant(constant) => match &constant.value {
                Constant::Int(value) => match i64::try_from(value.clone()) {
                    Ok(literal) => self.builder.int(literal, origin),
                    // Python integers are unbounded; the fragment's are not.
                    // Truncating would change the program, so it refuses.
                    Err(_) => self
                        .builder
                        .unsupported_expr(Construct::ArithmeticOverflow, origin),
                },
                // `True` is `1` and `False` is `0`, exactly, in Python.
                Constant::Bool(flag) => self.builder.int(i64::from(*flag), origin),
                Constant::Str(_) | Constant::Bytes(_) | Constant::Tuple(_) => {
                    self.builder.unsupported_expr(Construct::Collection, origin)
                }
                _ => self
                    .builder
                    .unsupported_expr(Construct::NonIntegerValue, origin),
            },

            Expr::Name(name) => self.read(name.id.as_str(), node),

            Expr::BinOp(binary) => {
                let Some(op) = arith_of(&binary.op) else {
                    // `x ** 2` is polynomial and `x ** y` is not, so the
                    // exponent decides, and only a small literal qualifies.
                    if matches!(binary.op, ast::Operator::Pow)
                        && let Some(exponent) = literal_exponent(&binary.right)
                    {
                        return match recall(&binary.left) {
                            Some(base) => self.builder.pow(base, exponent, origin),
                            None => self
                                .builder
                                .unsupported_expr(Construct::NonPolynomialPower, origin),
                        };
                    }
                    return self
                        .builder
                        .unsupported_expr(refusal_for(&binary.op), origin);
                };
                match (recall(&binary.left), recall(&binary.right)) {
                    (Some(left), Some(right)) => self.builder.arith(op, left, right, origin),
                    _ => self
                        .builder
                        .unsupported_expr(Construct::NonIntegerValue, origin),
                }
            }

            Expr::UnaryOp(unary) => match unary.op {
                ast::UnaryOp::USub => match recall(&unary.operand) {
                    Some(operand) => self.builder.neg(operand, origin),
                    None => self
                        .builder
                        .unsupported_expr(Construct::NonIntegerValue, origin),
                },
                ast::UnaryOp::UAdd => recall(&unary.operand).unwrap_or_else(|| {
                    self.builder
                        .unsupported_expr(Construct::NonIntegerValue, origin)
                }),
                ast::UnaryOp::Invert => self
                    .builder
                    .unsupported_expr(Construct::BitwiseOperator, origin),
                ast::UnaryOp::Not => self
                    .builder
                    .unsupported_expr(Construct::ConditionalExpression, origin),
            },

            Expr::Call(call) => {
                let detail = match call.func.as_ref() {
                    Expr::Name(name) => name.id.to_string(),
                    Expr::Attribute(attribute) => attribute.attr.to_string(),
                    _ => "call".to_owned(),
                };
                self.builder
                    .unsupported_expr_detailed(Construct::Call, detail, origin)
            }
            Expr::Attribute(attribute) => self.builder.unsupported_expr_detailed(
                Construct::Attribute,
                attribute.attr.as_str(),
                origin,
            ),
            Expr::Subscript(_) | Expr::Slice(_) | Expr::Starred(_) => {
                self.builder.unsupported_expr(Construct::Subscript, origin)
            }
            Expr::List(_)
            | Expr::Tuple(_)
            | Expr::Set(_)
            | Expr::Dict(_)
            | Expr::JoinedStr(_)
            | Expr::FormattedValue(_) => {
                self.builder.unsupported_expr(Construct::Collection, origin)
            }
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::GeneratorExp(_) => {
                self.builder
                    .unsupported_expr(Construct::Comprehension, origin)
            }
            Expr::Lambda(_) => self
                .builder
                .unsupported_expr(Construct::Declaration, origin),
            Expr::Await(_) | Expr::Yield(_) | Expr::YieldFrom(_) => {
                self.builder.unsupported_expr(Construct::Coroutine, origin)
            }
            Expr::NamedExpr(_) => self
                .builder
                .unsupported_expr(Construct::BindingForm, origin),
            Expr::IfExp(_) | Expr::BoolOp(_) | Expr::Compare(_) => self
                .builder
                .unsupported_expr(Construct::ConditionalExpression, origin),
        }
    }

    /// Reads a variable, or refuses if it is not a proven integer.
    fn read<T: Ranged>(&mut self, name: &str, node: &T) -> ExprId {
        let origin = self.origin(node);
        if self.integers.contains(name) {
            return self.builder.var(VarName::new(name), origin);
        }
        self.builder
            .unsupported_expr_detailed(Construct::NonIntegerValue, name, origin)
    }
}

/// A `range` step that keeps the loop in the fragment.
fn literal_step(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Constant(constant) => match &constant.value {
            Constant::Int(value) => i64::try_from(value.clone()).ok(),
            _ => None,
        },
        Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::USub) => {
            literal_step(&unary.operand)?.checked_neg()
        }
        _ => None,
    }
}

/// The fragment's comparison for a Python one, if it has one.
const fn compare_of(op: &ast::CmpOp) -> Option<CompareOp> {
    match op {
        ast::CmpOp::Lt => Some(CompareOp::Lt),
        ast::CmpOp::LtE => Some(CompareOp::Le),
        ast::CmpOp::Gt => Some(CompareOp::Gt),
        ast::CmpOp::GtE => Some(CompareOp::Ge),
        ast::CmpOp::Eq => Some(CompareOp::Eq),
        ast::CmpOp::NotEq => Some(CompareOp::Ne),
        ast::CmpOp::In | ast::CmpOp::NotIn | ast::CmpOp::Is | ast::CmpOp::IsNot => None,
    }
}

/// The children of an expression that the fragment translates.
///
/// Only the forms that survive into the fragment have children here. A refused
/// form has none, because it becomes one `Unsupported` node and its interior is
/// never inspected -- which is also what keeps a refused comprehension from
/// producing a refusal per node inside it.
fn expression_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BinOp(binary) => match binary.op {
            ast::Operator::Add | ast::Operator::Sub | ast::Operator::Mult => {
                vec![&binary.left, &binary.right]
            }
            // Only the base: the exponent must be a literal, read directly.
            ast::Operator::Pow => vec![&binary.left],
            _ => Vec::new(),
        },
        Expr::UnaryOp(unary) => match unary.op {
            ast::UnaryOp::USub | ast::UnaryOp::UAdd => vec![&unary.operand],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The sub-*conditions* of a condition.
fn condition_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BoolOp(boolean) => boolean.values.iter().collect(),
        Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => vec![&unary.operand],
        _ => Vec::new(),
    }
}

/// Every node reachable from `root`, children before parents.
///
/// Explicitly worklist-driven. Expression depth is capped at
/// `MAX_EXPRESSION_DEPTH`, which is ten thousand operators -- deep enough that
/// a recursive translation of a generated file risks the stack, and a stack
/// overflow is an abort that no lint can see.
fn postorder<'e>(root: &'e Expr, children: fn(&'e Expr) -> Vec<&'e Expr>) -> Vec<&'e Expr> {
    let mut ordered = Vec::new();
    let mut work = vec![root];
    while let Some(node) = work.pop() {
        ordered.push(node);
        work.extend(children(node));
    }
    // Reversing a parents-then-children order yields children before parents,
    // so every child's identifier is in the map before its parent is built.
    ordered.reverse();
    ordered
}

/// The position of a node, as [`Location`].
fn position<T: Ranged>(path: &Path, index: &LineIndex, node: &T) -> Location {
    let (line, column) = index.position(node.start().to_usize());
    Location::new(path.to_path_buf(), line, column)
}

/// The position of a node, as an opaque [`Origin`] for Core.
fn origin_of<T: Ranged>(path: &Path, index: &LineIndex, node: &T) -> Origin {
    let (line, column) = index.position(node.start().to_usize());
    Origin::new(format!("{}:{line}:{column}", path.display()))
}
