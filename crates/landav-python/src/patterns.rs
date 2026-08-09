//! The eleven `F-005` pattern rules.
//!
//! # The shape every rule shares
//!
//! Each one asks the same three questions in the same order:
//!
//! 1. *Does the enclosing loop run an unbounded number of times?* A fixed
//!    three-element loop makes any per-iteration cost a constant.
//! 2. *Is the per-iteration cost linear in something that grows?* That is the
//!    pattern itself.
//! 3. *Is there a cheaper spelling with the same meaning?* If rewriting changes
//!    the result rather than the cost, the rule must stay silent — the finding
//!    would be advice to introduce a bug.
//!
//! Question 3 is where the false-positive budget is spent, and it is the reason
//! several rules below look narrower than their names suggest.

use std::collections::BTreeSet;

use rustpython_parser::ast::{CmpOp, Expr, ExprContext, Operator, Ranged, Stmt};

use crate::{
    context::{
        Bindings, LoopInfo, StmtCtx, body_reads_a_subscript, contains, depends_on, is_call_of,
        is_scanned_list, is_str_expr, slice_of,
    },
    finding::Finding,
    location::Location,
    rule_code::RuleCode,
    syntax::{
        LineIndex, expr_tree, free_names, integer_literal, is_zero_literal, name_of,
        stmt_own_exprs, stmt_tree,
    },
};

/// How far `is_str_expr` and friends chase a binding. See `context`.
const TYPE_DEPTH: u32 = 4;

/// Names whose call builds a whole collection eagerly, for `LAV007`.
const COLLECTION_BUILDERS: [&str; 5] = ["set", "dict", "list", "frozenset", "tuple"];

/// Methods that mutate the receiver in place, for `LAV007`'s hoisting check.
const MUTATORS: [&str; 11] = [
    "add",
    "append",
    "extend",
    "insert",
    "update",
    "discard",
    "remove",
    "pop",
    "setdefault",
    "clear",
    "sort",
];

/// The `re` entry points that scan a subject from the start.
const REGEX_SCANS: [&str; 8] = [
    "search",
    "match",
    "fullmatch",
    "findall",
    "finditer",
    "sub",
    "subn",
    "split",
];

/// The exceptions `LAV010` will act on.
///
/// Both have a total, non-raising alternative that is never slower —
/// `dict.get`, or a membership test. `OSError` deliberately does not: the
/// "look before you leap" rewrite of `open` is a TOCTOU bug, so a handler is
/// the only correct spelling and the rule has nothing to suggest.
const RECOVERABLE_LOOKUP_ERRORS: [&str; 2] = ["KeyError", "IndexError"];

/// Everything one file's rules are run against.
pub(crate) struct Analysis<'a> {
    pub(crate) path: &'a std::path::Path,
    pub(crate) source: &'a str,
    pub(crate) index: LineIndex,
    pub(crate) bindings: Bindings<'a>,
    pub(crate) loops: Vec<LoopInfo<'a>>,
    pub(crate) statements: Vec<StmtCtx<'a>>,
}

impl<'a> Analysis<'a> {
    /// A finding pointing at a byte offset in this file.
    fn at<T: Ranged>(&self, code: RuleCode, node: &T, explanation: &str) -> Finding {
        let (line, column) = self.index.position(node.range().start().to_usize());
        Finding::new(
            code,
            Location::new(self.path.to_path_buf(), line, column),
            explanation.to_owned(),
        )
    }

    /// The innermost enclosing loop, but only when its trip count is unbounded.
    fn hot_loop(&self, ctx: &StmtCtx<'a>) -> Option<&LoopInfo<'a>> {
        let index = ctx.innermost()?;
        let info = self.loops.get(index)?;
        (!info.bounded).then_some(info)
    }

    /// Every loop enclosing `ctx`, innermost last.
    fn enclosing(&self, ctx: &StmtCtx<'a>) -> Vec<&LoopInfo<'a>> {
        ctx.loops
            .iter()
            .filter_map(|index| self.loops.get(*index))
            .collect()
    }

    /// Every expression the statement evaluates itself, at any depth.
    fn exprs_of(&self, stmt: &'a Stmt) -> Vec<&'a Expr> {
        let mut own = Vec::new();
        stmt_own_exprs(stmt, &mut own);
        own.into_iter().flat_map(expr_tree).collect()
    }

    /// Runs every rule and returns the findings in `(line, column, code)` order.
    pub(crate) fn run(&self) -> Vec<Finding> {
        let mut findings = Vec::new();

        for ctx in &self.statements {
            lav001(self, ctx, &mut findings);
            lav002(self, ctx, &mut findings);
            lav003(self, ctx, &mut findings);
            lav004(self, ctx, &mut findings);
            lav005(self, ctx, &mut findings);
            lav006(self, ctx, &mut findings);
            lav007(self, ctx, &mut findings);
            lav008(self, ctx, &mut findings);
            lav009(self, ctx, &mut findings);
            lav010(self, ctx, &mut findings);
            lav011(self, ctx, &mut findings);
        }

        findings.sort_by(|left, right| {
            let key = |finding: &Finding| {
                (
                    finding.location().line(),
                    finding.location().column(),
                    finding.rule().as_str(),
                )
            };
            key(left).cmp(&key(right))
        });
        findings.dedup_by(|left, right| {
            left.rule() == right.rule() && left.location() == right.location()
        });
        findings
    }
}

/// `LAV001` — `list.index()` inside a loop over an unbounded sequence.
fn lav001(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }
    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Call(call) = expr else { continue };
        let Expr::Attribute(func) = call.func.as_ref() else {
            continue;
        };
        if func.attr.as_str() == "index" && !call.args.is_empty() {
            out.push(analysis.at(
                crate::registry::LAV001,
                call,
                "list.index() scans from the front on every iteration; build a position map once",
            ));
        }
    }
}

/// `LAV002` — `in` against a list inside a loop.
fn lav002(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }
    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Compare(compare) = expr else {
            continue;
        };
        let membership = compare
            .ops
            .iter()
            .enumerate()
            .find(|(_, op)| matches!(op, CmpOp::In | CmpOp::NotIn));
        let Some((position, _)) = membership else {
            continue;
        };
        let Some(container) = compare.comparators.get(position) else {
            continue;
        };
        if is_scanned_list(container, &analysis.bindings, TYPE_DEPTH) {
            out.push(analysis.at(
                crate::registry::LAV002,
                compare,
                "membership against a list is a linear scan per iteration; use a set or dict",
            ));
        }
    }
}

/// `LAV003` — a `str` accumulated with `+=` across an unbounded loop.
fn lav003(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };

    let accumulator = match ctx.stmt {
        Stmt::AugAssign(node) if matches!(node.op, Operator::Add) => {
            name_of(node.target.as_ref()).map(str::to_owned)
        }
        Stmt::Assign(node) => match node.targets.as_slice() {
            [target] => name_of(target)
                .filter(|name| leftmost_addend(node.value.as_ref()) == Some(*name))
                .map(str::to_owned),
            _ => None,
        },
        _ => None,
    };
    let Some(name) = accumulator else { return };

    if !is_string_accumulator(analysis, info, &name) {
        return;
    }

    out.push(analysis.at(
        crate::registry::LAV003,
        ctx.stmt,
        "each += copies the whole string, so the loop is quadratic; append to a list and join once",
    ));
}

/// The name at the far left of a chain of `+`, or `None`.
fn leftmost_addend(expr: &Expr) -> Option<&str> {
    let mut current = expr;
    loop {
        match current {
            Expr::BinOp(node) if matches!(node.op, Operator::Add) => current = node.left.as_ref(),
            _ => return name_of(current),
        }
    }
}

/// `true` when `name` is initialised to a `str` outside the loop and never
/// reset inside it.
///
/// The reset check is the whole rule: `line = ""` at the top of the body means
/// the string is per-iteration and the total work is linear, which is the
/// idiom `per_iteration_string_is_not_an_accumulator.py` is built from.
fn is_string_accumulator(analysis: &Analysis<'_>, info: &LoopInfo<'_>, name: &str) -> bool {
    let mut initialised_outside = false;

    for ctx in &analysis.statements {
        let Stmt::Assign(node) = ctx.stmt else {
            continue;
        };
        let [target] = node.targets.as_slice() else {
            continue;
        };
        if name_of(target) != Some(name) {
            continue;
        }

        let rebinds = !free_names(node.value.as_ref())
            .iter()
            .any(|free| free == name);
        if contains(info.stmt, ctx.stmt) {
            if rebinds {
                return false;
            }
        } else if rebinds && is_str_expr(node.value.as_ref(), &analysis.bindings, TYPE_DEPTH) {
            initialised_outside = true;
        }
    }

    initialised_outside
}

/// `LAV004` — `insert(0, …)` or `pop(0)` on a list inside a loop.
fn lav004(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }
    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Call(call) = expr else { continue };
        let Expr::Attribute(func) = call.func.as_ref() else {
            continue;
        };

        let front = match func.attr.as_str() {
            "insert" => call.args.first().is_some_and(is_zero_literal),
            "pop" => call.args.len() == 1 && call.args.first().is_some_and(is_zero_literal),
            _ => false,
        };
        if !front {
            continue;
        }
        // A `deque` shifts nothing, so the same spelling is O(1) on one.
        if is_call_of(func.value.as_ref(), &analysis.bindings, "deque") {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV004,
            call,
            "mutating the front of a list shifts every remaining element; use collections.deque",
        ));
    }
}

/// `LAV005` — a loop nested inside another loop over the same collection.
fn lav005(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let iter = match ctx.stmt {
        Stmt::For(node) => node.iter.as_ref(),
        Stmt::AsyncFor(node) => node.iter.as_ref(),
        _ => return,
    };
    let Some(text) = slice_of(analysis.source, iter) else {
        return;
    };

    let inner_is_bounded = analysis
        .loops
        .iter()
        .find(|info| std::ptr::eq(info.stmt, ctx.stmt))
        .is_some_and(|info| info.bounded);
    if inner_is_bounded {
        return;
    }

    let repeats = analysis
        .enclosing(ctx)
        .iter()
        .any(|outer| outer.iter_text == Some(text) && !outer.bounded);
    if repeats {
        out.push(analysis.at(
            crate::registry::LAV005,
            ctx.stmt,
            "both loops walk the same collection, so the pair costs n squared comparisons",
        ));
    }
}

/// `LAV006` — sorting a list that the same loop is still appending to.
fn lav006(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };

    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Call(call) = expr else { continue };

        let subject = match call.func.as_ref() {
            Expr::Attribute(func) if func.attr.as_str() == "sort" => name_of(func.value.as_ref()),
            Expr::Name(func) if func.id.as_str() == "sorted" => call.args.first().and_then(name_of),
            _ => None,
        };
        let Some(name) = subject else { continue };

        if !accumulates_in(info, name) || !built_outside(analysis, info, name) {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV006,
            call,
            "re-sorting a growing list on every append costs n^2 log n; sort once after the loop",
        ));
    }
}

/// `true` when the loop body appends to `name`.
fn accumulates_in(info: &LoopInfo<'_>, name: &str) -> bool {
    for stmt in stmt_tree(info.body) {
        match stmt {
            Stmt::AugAssign(node) if name_of(node.target.as_ref()) == Some(name) => return true,
            Stmt::Expr(node) => {
                if let Expr::Call(call) = node.value.as_ref()
                    && let Expr::Attribute(func) = call.func.as_ref()
                    && name_of(func.value.as_ref()) == Some(name)
                    && matches!(func.attr.as_str(), "append" | "extend" | "insert")
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// `true` when `name` is bound to a fresh list before the loop starts.
fn built_outside(analysis: &Analysis<'_>, info: &LoopInfo<'_>, name: &str) -> bool {
    analysis.statements.iter().any(|ctx| {
        let Stmt::Assign(node) = ctx.stmt else {
            return false;
        };
        let [target] = node.targets.as_slice() else {
            return false;
        };
        name_of(target) == Some(name)
            && !contains(info.stmt, ctx.stmt)
            && matches!(node.value.as_ref(), Expr::List(_) | Expr::ListComp(_))
    })
}

/// `LAV007` — a collection rebuilt every iteration from loop-invariant inputs.
fn lav007(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };
    let Stmt::Assign(node) = ctx.stmt else { return };
    let [target] = node.targets.as_slice() else {
        return;
    };
    let Some(name) = name_of(target) else { return };
    let value = node.value.as_ref();

    if !is_eager_collection_build(value) {
        return;
    }

    // `y = tuple(y)` reads the previous iteration's value; there is nothing
    // invariant about it.
    if free_names(value).iter().any(|read| read == name) {
        return;
    }

    // A call inside the build may consume or mutate — `list(islice(it, n))`
    // advances the iterator, so hoisting it does not repeat the work, it
    // deletes most of it. Only a call-free build is provably safe to move.
    if !build_is_side_effect_free(value) {
        return;
    }

    // Anything the loop varies makes the build genuinely per-iteration.
    let mut varying: BTreeSet<String> = info.targets.clone();
    for outer in analysis.enclosing(ctx) {
        varying.extend(outer.targets.iter().cloned());
    }
    varying.extend(info.assigned.iter().cloned());
    varying.remove(name);
    if depends_on(value, &varying) {
        return;
    }

    // Hoisting a collection the body mutates changes the result, not the cost:
    // `seen = set()` per group is the whole point of `seen`.
    if mutated_in(info, name) || assigned_more_than_once(info, name) {
        return;
    }

    out.push(analysis.at(
        crate::registry::LAV007,
        value,
        "this collection does not depend on the loop variable, so it is rebuilt once per iteration",
    ));
}

/// `true` for a collection constructor call with arguments, or a comprehension.
///
/// A no-argument `set()` is excluded on purpose: it is a fresh accumulator, and
/// hoisting one merges state that was meant to be per-iteration.
fn is_eager_collection_build(expr: &Expr) -> bool {
    match expr {
        Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) => true,
        Expr::Call(call) => {
            name_of(call.func.as_ref()).is_some_and(|func| COLLECTION_BUILDERS.contains(&func))
                && !call.args.is_empty()
        }
        _ => false,
    }
}

/// `true` when nothing inside the build can have an effect of its own.
///
/// The outermost constructor is exempt — that call *is* the build. Anything
/// nested is not: a generator, an iterator or a method call may be stateful,
/// and moving a stateful expression out of a loop changes what the program
/// does. The rule would rather miss a hoistable build than propose one.
fn build_is_side_effect_free(expr: &Expr) -> bool {
    let inner: Vec<&Expr> = match expr {
        Expr::Call(call) => call
            .args
            .iter()
            .chain(call.keywords.iter().map(|keyword| &keyword.value))
            .collect(),
        other => vec![other],
    };
    !inner
        .into_iter()
        .flat_map(expr_tree)
        .any(|node| matches!(node, Expr::Call(_)))
}

/// `true` when the loop body mutates `name` in place or writes through it.
fn mutated_in(info: &LoopInfo<'_>, name: &str) -> bool {
    for stmt in stmt_tree(info.body) {
        if let Stmt::AugAssign(node) = stmt
            && name_of(node.target.as_ref()) == Some(name)
        {
            return true;
        }
        if let Stmt::Assign(node) = stmt
            && node.targets.iter().any(|target| match target {
                Expr::Subscript(sub) => name_of(sub.value.as_ref()) == Some(name),
                _ => false,
            })
        {
            return true;
        }

        let mut own = Vec::new();
        stmt_own_exprs(stmt, &mut own);
        for expr in own.into_iter().flat_map(expr_tree) {
            if let Expr::Call(call) = expr
                && let Expr::Attribute(func) = call.func.as_ref()
                && name_of(func.value.as_ref()) == Some(name)
                && MUTATORS.contains(&func.attr.as_str())
            {
                return true;
            }
        }
    }
    false
}

/// `true` when the loop body binds `name` in more than one place.
fn assigned_more_than_once(info: &LoopInfo<'_>, name: &str) -> bool {
    let count = stmt_tree(info.body)
        .into_iter()
        .filter(|stmt| match stmt {
            Stmt::Assign(node) => node
                .targets
                .iter()
                .any(|target| name_of(target) == Some(name)),
            _ => false,
        })
        .count();
    count > 1
}

/// `LAV008` — a slice whose length grows with the loop, taken every iteration.
fn lav008(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };

    let mut targets: BTreeSet<String> = info.targets.clone();
    for outer in analysis.enclosing(ctx) {
        targets.extend(outer.targets.iter().cloned());
    }
    let mut varying = targets.clone();
    varying.extend(info.assigned.iter().cloned());

    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Subscript(subscript) = expr else {
            continue;
        };
        let Expr::Slice(slice) = subscript.slice.as_ref() else {
            continue;
        };
        // A slice *assignment* is not a copy of the span; `xs[i:i + 1] = ys`
        // splices, and the cost model is the list's, not the slice's.
        if !matches!(subscript.ctx, ExprContext::Load) {
            continue;
        }
        if is_fixed_width(slice, analysis.source, &varying) {
            continue;
        }
        // A slice of the item the loop is currently on copies that item, not the
        // whole input, so the loop total stays linear.
        if depends_on(subscript.value.as_ref(), &targets) {
            continue;
        }
        // `memoryview` slices share storage; nothing is copied.
        if is_call_of(subscript.value.as_ref(), &analysis.bindings, "memoryview") {
            continue;
        }
        // The span has to be shown to grow, not merely to be a slice. Either
        // the loop consumes the object by reslicing it into itself, or a bound
        // is driven by the loop variable. Every other slice in a loop copies
        // something whose size this pass cannot see, and reporting those is
        // what makes a slice rule fire on half a repository.
        if !(consumes_by_reslicing(analysis, ctx, subscript) || is_driven_by(slice, &targets)) {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV008,
            subscript,
            "this slice copies a span that grows with the loop; keep an index or a memoryview",
        ));
    }
}

/// `true` when the slice's length does not grow with the loop.
///
/// Three shapes qualify, and all three are common enough that missing any one
/// of them makes a slice rule unusable on real code:
///
/// * literal bounds — `line[:19]`;
/// * a negative literal lower with no upper — `line[-1:]` is one element,
///   however long `line` is;
/// * a window whose *difference* is loop-invariant — `buf[i:i + 4]`,
///   `self[i:i + width]`. Neither bound is a constant, but the length is, and
///   this is how every binary-format parser is written.
fn is_fixed_width(
    slice: &rustpython_parser::ast::ExprSlice,
    source: &str,
    varying: &BTreeSet<String>,
) -> bool {
    let lower = slice.lower.as_deref();

    let Some(upper) = slice.upper.as_deref() else {
        // `buf[-k:]` keeps the last k elements. Anything else open-ended copies
        // whatever is left, and "whatever is left" is what the loop consumes.
        return lower.is_some_and(is_negative_literal);
    };

    if let Some(upper) = integer_literal(upper) {
        return match lower {
            None => true,
            Some(lower) => integer_literal(lower).is_some_and(|lower| lower <= upper),
        };
    }

    let Some(lower) = lower.and_then(|lower| slice_of(source, lower)) else {
        return false;
    };
    let Expr::BinOp(offset) = upper else {
        return false;
    };
    if !matches!(offset.op, Operator::Add) {
        return false;
    }

    let widens_by_a_fixed_amount = |base: &Expr, width: &Expr| {
        slice_of(source, base) == Some(lower) && !depends_on(width, varying)
    };
    widens_by_a_fixed_amount(offset.left.as_ref(), offset.right.as_ref())
        || widens_by_a_fixed_amount(offset.right.as_ref(), offset.left.as_ref())
}

/// `true` when the statement rebinds the sliced object to a slice of itself.
///
/// `buffer = buffer[n:]` is the buffer-consumption idiom: each pass copies the
/// remainder, so draining an n-byte buffer copies O(n^2) bytes.
fn consumes_by_reslicing(
    analysis: &Analysis<'_>,
    ctx: &StmtCtx<'_>,
    subscript: &rustpython_parser::ast::ExprSubscript,
) -> bool {
    let target = match ctx.stmt {
        Stmt::Assign(node) => match node.targets.as_slice() {
            [target] => target,
            _ => return false,
        },
        Stmt::AugAssign(node) => node.target.as_ref(),
        _ => return false,
    };
    let sliced = slice_of(analysis.source, subscript.value.as_ref());
    sliced.is_some() && slice_of(analysis.source, target) == sliced
}

/// `true` when a slice bound is driven by an enclosing loop's variable, which
/// is what makes the span grow from one iteration to the next.
fn is_driven_by(slice: &rustpython_parser::ast::ExprSlice, targets: &BTreeSet<String>) -> bool {
    [
        slice.lower.as_deref(),
        slice.upper.as_deref(),
        slice.step.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|bound| depends_on(bound, targets))
}

/// `true` for `-1`, `-8` and friends, which the parser sees as a unary minus.
fn is_negative_literal(expr: &Expr) -> bool {
    match expr {
        Expr::UnaryOp(node) => {
            matches!(node.op, rustpython_parser::ast::UnaryOp::USub)
                && integer_literal(node.operand.as_ref()).is_some()
        }
        _ => false,
    }
}

/// `LAV009` — a dataframe grown one row or one frame at a time.
fn lav009(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }
    let Stmt::Assign(node) = ctx.stmt else { return };
    let [target] = node.targets.as_slice() else {
        return;
    };
    let Some(name) = name_of(target) else { return };
    let Expr::Call(call) = node.value.as_ref() else {
        return;
    };

    let grows = match call.func.as_ref() {
        // `frame = frame.append(row)` — `list.append` returns `None`, so an
        // assignment back to the receiver is never a list.
        Expr::Attribute(func) if func.attr.as_str() == "append" => {
            name_of(func.value.as_ref()) == Some(name)
        }
        // `frame = pd.concat([frame, chunk])`.
        Expr::Attribute(func) if func.attr.as_str() == "concat" => concat_reuses(call, name),
        Expr::Name(func) if func.id.as_str() == "concat" => concat_reuses(call, name),
        _ => false,
    };

    if grows {
        out.push(analysis.at(
            crate::registry::LAV009,
            call,
            "every concat copies the whole accumulated frame; collect the parts and concat once",
        ));
    }
}

/// `true` when the first argument of a concat is a sequence containing `name`.
fn concat_reuses(call: &rustpython_parser::ast::ExprCall, name: &str) -> bool {
    let Some(first) = call.args.first() else {
        return false;
    };
    let elements = match first {
        Expr::List(node) => node.elts.as_slice(),
        Expr::Tuple(node) => node.elts.as_slice(),
        _ => return false,
    };
    elements
        .iter()
        .any(|element| name_of(element) == Some(name))
}

/// `LAV010` — an exception used as control flow inside a loop.
///
/// # The narrowing, and why it has to be this tight
///
/// Setting up `try` is free in CPython; only *raising* costs, and how often a
/// loop raises is a runtime property no static pass can see. So the rule does
/// not report `try` in a loop. It reports the statically decidable proxy:
///
/// * the handler's sole effect is `continue`, `pass`, or a default assignment —
///   which is what "the exception is the branch" looks like; and
/// * the guarded operation is a subscript, for which a total, never-slower
///   alternative exists (`dict.get`, a membership test); and
/// * the exception is a lookup error, not an I/O error.
///
/// `open()` fails all three, and must: `os.path.exists` before `open` is a
/// TOCTOU bug, so the handler is the only correct spelling however often it
/// fires.
fn lav010(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }
    let Stmt::Try(node) = ctx.stmt else { return };
    if node.handlers.is_empty() || !node.finalbody.is_empty() {
        return;
    }
    if !body_reads_a_subscript(&node.body) {
        return;
    }

    let every_handler_is_a_branch = node.handlers.iter().all(|handler| {
        let rustpython_parser::ast::ExceptHandler::ExceptHandler(handler) = handler;
        handler
            .type_
            .as_deref()
            .is_some_and(is_recoverable_lookup_error)
            && handler_is_pure_branch(&handler.body)
    });

    if every_handler_is_a_branch {
        out.push(analysis.at(
            crate::registry::LAV010,
            ctx.stmt,
            "the handler is the branch, so every miss pays an unwind; dict.get expresses it directly",
        ));
    }
}

/// `true` for `KeyError`, `IndexError`, or a tuple of only those.
fn is_recoverable_lookup_error(expr: &Expr) -> bool {
    match expr {
        Expr::Tuple(node) => {
            !node.elts.is_empty() && node.elts.iter().all(is_recoverable_lookup_error)
        }
        _ => name_of(expr).is_some_and(|name| RECOVERABLE_LOOKUP_ERRORS.contains(&name)),
    }
}

/// `true` when a handler body does nothing but redirect control or store a
/// default — no calls, no logging, no bookkeeping.
fn handler_is_pure_branch(body: &[Stmt]) -> bool {
    if body.is_empty() {
        return false;
    }
    body.iter().all(|stmt| match stmt {
        Stmt::Continue(_) | Stmt::Pass(_) => true,
        Stmt::Assign(node) => {
            node.targets.iter().all(|target| name_of(target).is_some())
                && !contains_call(node.value.as_ref())
        }
        _ => false,
    })
}

fn contains_call(expr: &Expr) -> bool {
    expr_tree(expr)
        .into_iter()
        .any(|node| matches!(node, Expr::Call(_)))
}

/// `LAV011` — a regex re-scanning a subject that the loop keeps re-deriving.
fn lav011(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    if analysis.hot_loop(ctx).is_none() {
        return;
    }

    let rebound = match ctx.stmt {
        Stmt::Assign(node) => match node.targets.as_slice() {
            [target] => name_of(target),
            _ => None,
        },
        _ => None,
    };

    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Call(call) = expr else { continue };
        let Expr::Attribute(func) = call.func.as_ref() else {
            continue;
        };
        if !REGEX_SCANS.contains(&func.attr.as_str()) {
            continue;
        }
        if !is_regex_handle(func.value.as_ref(), &analysis.bindings) {
            continue;
        }

        let arguments: Vec<&Expr> = call
            .args
            .iter()
            .chain(call.keywords.iter().map(|keyword| &keyword.value))
            .collect();

        // A freshly sliced subject is a copy plus a rescan of the same tail.
        let scans_a_copy = arguments.iter().any(|argument| match argument {
            Expr::Subscript(node) => matches!(node.slice.as_ref(), Expr::Slice(_)),
            _ => false,
        });
        // `text = re.sub(..., text)` repeats whole passes until a fixpoint.
        let to_fixpoint = rebound.is_some_and(|name| {
            arguments
                .iter()
                .any(|argument| name_of(argument) == Some(name))
        });

        if scans_a_copy || to_fixpoint {
            out.push(analysis.at(
                crate::registry::LAV011,
                call,
                "the pattern rescans the subject from the start each pass; use finditer or a pos offset",
            ));
        }
    }
}

/// `true` for `re` itself or a name bound to `re.compile(...)`.
fn is_regex_handle(expr: &Expr, bindings: &Bindings<'_>) -> bool {
    name_of(expr) == Some("re") || is_call_of(expr, bindings, "compile")
}
