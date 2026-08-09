//! The `F-005` pattern rules.
//!
//! # The shape every rule shares
//!
//! Each one asks the same four questions in the same order:
//!
//! 1. *Does the enclosing loop run an unbounded number of times?* A fixed
//!    three-element loop makes any per-iteration cost a constant.
//! 2. *Is the object the loop's subject, or something the loop just made?*
//!    `cells = row.split(",")` is a fresh list per row; scanning it costs its
//!    own length, and summed over the loop that is the size of the input.
//! 3. *Is the per-iteration cost linear in something that grows?* That is the
//!    pattern itself, and "grows" is stricter than "is not constant" — a window
//!    that *moves* across a buffer copies each byte once in total.
//! 4. *Is there a cheaper spelling with the same meaning?* If rewriting changes
//!    the result rather than the cost, the rule must stay silent — the finding
//!    would be advice to introduce a bug.
//!
//! Questions 2 and 4 are where the false-positive budget is spent, and they are
//! the reason several rules below look narrower than their names suggest.

use std::collections::BTreeSet;

use rustpython_parser::ast::{CmpOp, Expr, ExprContext, Operator, Ranged, Stmt};

use crate::{
    context::{
        Bindings, LoopInfo, Program, RESOLUTION_DEPTH, StmtCtx, contains, depends_on,
        escapes_the_iteration, integer_constant, is_call_of, is_one_shot_iterator, is_scanned_list,
        is_short_constant_sequence, is_str_expr, slice_of,
    },
    finding::Finding,
    location::Location,
    rule_code::RuleCode,
    syntax::{
        LineIndex, expr_tree, free_names, integer_literal, is_zero_literal, name_of,
        stmt_own_exprs, stmt_tree,
    },
};

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

/// Everything one file's rules are run against.
pub(crate) struct Analysis<'a> {
    pub(crate) path: &'a std::path::Path,
    pub(crate) source: &'a str,
    pub(crate) index: LineIndex,
    pub(crate) program: Program<'a>,
}

impl<'a> Analysis<'a> {
    fn bindings(&self) -> &Bindings<'a> {
        &self.program.bindings
    }

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
        let info = self.program.loops.get(index)?;
        (!info.bounded).then_some(info)
    }

    /// Every loop enclosing `ctx`, innermost last.
    fn enclosing(&self, ctx: &StmtCtx<'a>) -> Vec<&LoopInfo<'a>> {
        ctx.loops
            .iter()
            .filter_map(|index| self.program.loops.get(*index))
            .collect()
    }

    /// Every expression the statement evaluates itself, at any depth.
    fn exprs_of(&self, stmt: &'a Stmt) -> Vec<&'a Expr> {
        let mut own = Vec::new();
        stmt_own_exprs(stmt, &mut own);
        own.into_iter().flat_map(expr_tree).collect()
    }

    /// The values `name` is assigned outside `info`, in the same scope.
    ///
    /// Scope matters more than it looks: a module-wide search reports
    /// `result += record.errors` as string concatenation because a *different
    /// function* happens to have a string called `result`.
    fn initialisers_outside(&self, info: &LoopInfo<'a>, scope: usize, name: &str) -> Vec<&'a Expr> {
        self.program
            .statements
            .iter()
            .filter(|ctx| ctx.scope == scope && !contains(info.stmt, ctx.stmt))
            .filter_map(|ctx| match ctx.stmt {
                Stmt::Assign(node) => match node.targets.as_slice() {
                    [target] if name_of(target) == Some(name) => Some(node.value.as_ref()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// Runs every rule and returns the findings in `(line, column, code)` order.
    pub(crate) fn run(&self) -> Vec<Finding> {
        let mut findings = Vec::new();

        for ctx in &self.program.statements {
            lav001(self, ctx, &mut findings);
            lav002(self, ctx, &mut findings);
            lav003(self, ctx, &mut findings);
            lav004(self, ctx, &mut findings);
            lav005(self, ctx, &mut findings);
            lav006(self, ctx, &mut findings);
            lav007(self, ctx, &mut findings);
            lav008(self, ctx, &mut findings);
            lav009(self, ctx, &mut findings);
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

/// `LAV001` — `list.index()` over a collection the loop did not just build.
fn lav001(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };
    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Call(call) = expr else { continue };
        let Expr::Attribute(func) = call.func.as_ref() else {
            continue;
        };
        if func.attr.as_str() != "index" || call.args.is_empty() {
            continue;
        }
        // `stripped.index("=")` on this line, or `cells.index(m)` on this row,
        // scans an object the loop just produced. Summed over the loop that is
        // one pass over the input, and there is no shared ordering to index.
        if info.varies_with(func.value.as_ref()) {
            continue;
        }
        // `str.index` walks one string, not one collection, and a string has no
        // position map to build. Different complexity class, same spelling.
        if is_str_expr(
            func.value.as_ref(),
            analysis.bindings(),
            ctx.scope,
            RESOLUTION_DEPTH,
        ) {
            continue;
        }
        // A short constant table is a bounded scan whatever encloses it.
        if is_short_constant_sequence(
            func.value.as_ref(),
            analysis.bindings(),
            ctx.scope,
            RESOLUTION_DEPTH,
        ) {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV001,
            call,
            "list.index() scans from the front on every iteration; build a position map once",
        ));
    }
}

/// `LAV002` — `in` against a list inside a loop.
fn lav002(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };
    for expr in analysis.exprs_of(ctx.stmt) {
        let Expr::Compare(compare) = expr else {
            continue;
        };
        let membership = compare
            .ops
            .iter()
            .position(|op| matches!(op, CmpOp::In | CmpOp::NotIn));
        let Some(position) = membership else {
            continue;
        };
        let Some(container) = compare.comparators.get(position) else {
            continue;
        };
        if info.varies_with(container) {
            continue;
        }
        if is_scanned_list(container, analysis.bindings(), ctx.scope, RESOLUTION_DEPTH) {
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

    if !is_string_accumulator(analysis, info, ctx.scope, &name) {
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

/// `true` when `name` is initialised to a `str` outside the loop, in this same
/// scope, and never reset inside it.
///
/// The reset check keeps a per-iteration string out: `line = ""` at the top of
/// the body means the total work is linear. The scope check keeps *another
/// function's* `result` out, which is the same rule applied to names instead of
/// to values.
fn is_string_accumulator(
    analysis: &Analysis<'_>,
    info: &LoopInfo<'_>,
    scope: usize,
    name: &str,
) -> bool {
    let mut initialised_outside = false;

    for ctx in &analysis.program.statements {
        if ctx.scope != scope {
            continue;
        }
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
        } else if rebinds
            && is_str_expr(
                node.value.as_ref(),
                analysis.bindings(),
                scope,
                RESOLUTION_DEPTH,
            )
        {
            initialised_outside = true;
        }
    }

    initialised_outside
}

/// `LAV004` — `insert(0, …)` or `pop(0)` on a list the loop drains.
fn lav004(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };
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
        // `parts.pop(0)` shifts this line's fields, not the whole file.
        if info.varies_with(func.value.as_ref()) {
            continue;
        }
        // A `deque` shifts nothing, so the same spelling is O(1) on one.
        if is_call_of(func.value.as_ref(), analysis.bindings(), ctx.scope, "deque") {
            continue;
        }
        // The quadratic claim needs the loop to keep going until the list is
        // empty. A `while` that also stops on a predicate — a leading-flag
        // parser — runs as many times as there are flags, not as there are
        // elements, so the shifts do not compound.
        if !drains_the_container(info, func.value.as_ref()) {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV004,
            call,
            "mutating the front of a list shifts every remaining element; use collections.deque",
        ));
    }
}

/// `true` when the loop is guaranteed to keep mutating until the list runs out.
fn drains_the_container(info: &LoopInfo<'_>, container: &Expr) -> bool {
    let test = match info.stmt {
        // An unbounded `for` performs one mutation per input element.
        Stmt::For(_) | Stmt::AsyncFor(_) => return true,
        Stmt::While(node) => node.test.as_ref(),
        _ => return false,
    };
    let Some(name) = name_of(container) else {
        return false;
    };

    let is_emptiness_of = |expr: &Expr| match expr {
        Expr::Name(node) => node.id.as_str() == name,
        Expr::Call(call) => {
            name_of(call.func.as_ref()) == Some("len")
                && call.args.first().and_then(name_of) == Some(name)
        }
        _ => false,
    };

    match test {
        Expr::Compare(node) => is_emptiness_of(node.left.as_ref()),
        other => is_emptiness_of(other),
    }
}

/// `LAV005` — a loop nested inside another loop over the same collection.
fn lav005(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let (iter, body) = match ctx.stmt {
        Stmt::For(node) => (node.iter.as_ref(), node.body.as_slice()),
        Stmt::AsyncFor(node) => (node.iter.as_ref(), node.body.as_slice()),
        _ => return,
    };
    let Some(text) = slice_of(analysis.source, iter) else {
        return;
    };

    let inner = analysis
        .program
        .loops
        .iter()
        .find(|info| std::ptr::eq(info.stmt, ctx.stmt));
    let Some(inner) = inner else { return };
    if inner.bounded {
        return;
    }

    // One iterator shared by both headers is consumed once between them, not
    // restarted by the inner loop. `for a in stream: for b in stream:` visits
    // every element exactly once in total.
    if is_one_shot_iterator(iter, analysis.bindings(), ctx.scope, RESOLUTION_DEPTH) {
        return;
    }

    let outer_targets: BTreeSet<String> = analysis
        .enclosing(ctx)
        .iter()
        .filter(|outer| outer.iter_text == Some(text) && !outer.bounded)
        .flat_map(|outer| outer.targets.iter().cloned())
        .collect();
    if outer_targets.is_empty() {
        return;
    }

    // The quadratic shape is a *pairing*: every outer item is examined against
    // every inner item. An inner loop that never mentions the outer variable is
    // not forming pairs — it is continuing the same traversal, which is what a
    // paragraph reader or a length-prefixed record reader does.
    if !body_reads_any(body, &outer_targets) {
        return;
    }

    out.push(analysis.at(
        crate::registry::LAV005,
        ctx.stmt,
        "both loops walk the same collection, so the pair costs n squared comparisons",
    ));
}

/// `true` when any expression in these statements reads one of `names`.
fn body_reads_any(body: &[Stmt], names: &BTreeSet<String>) -> bool {
    let mut own = Vec::new();
    for stmt in stmt_tree(body) {
        stmt_own_exprs(stmt, &mut own);
    }
    own.into_iter().any(|expr| depends_on(expr, names))
}

/// `LAV006` — sorting a list the same loop is still appending to.
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

        if !accumulates_in(info, name) {
            continue;
        }
        let built_outside = analysis
            .initialisers_outside(info, ctx.scope, name)
            .iter()
            .any(|value| matches!(value, Expr::List(_) | Expr::ListComp(_)));
        if !built_outside {
            continue;
        }
        // A list emptied every batch, or trimmed to a constant every pass, never
        // grows: each sort is O(k log k) for a fixed k, and "sort once after the
        // loop" would change what the code produces.
        if resets_inside(info, name) || bounded_by_a_constant(analysis, info, ctx.scope, name) {
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

/// `true` when the body rebinds `name` to a fresh empty list.
fn resets_inside(info: &LoopInfo<'_>, name: &str) -> bool {
    stmt_tree(info.body).into_iter().any(|stmt| match stmt {
        Stmt::Assign(node) => match node.targets.as_slice() {
            [target] => {
                name_of(target) == Some(name)
                    && matches!(node.value.as_ref(), Expr::List(list) if list.elts.is_empty())
            }
            _ => false,
        },
        _ => false,
    })
}

/// `true` when the body caps `name`'s length at a compile-time constant.
fn bounded_by_a_constant(
    analysis: &Analysis<'_>,
    info: &LoopInfo<'_>,
    scope: usize,
    name: &str,
) -> bool {
    let constant = |expr: &Expr| {
        integer_constant(expr, analysis.bindings(), scope, RESOLUTION_DEPTH).is_some()
    };
    let truncation = |target: &Expr| match target {
        Expr::Subscript(node) => {
            name_of(node.value.as_ref()) == Some(name)
                && match node.slice.as_ref() {
                    Expr::Slice(slice) => {
                        slice.lower.as_deref().is_some_and(&constant)
                            || slice.upper.as_deref().is_some_and(&constant)
                    }
                    _ => false,
                }
        }
        _ => false,
    };

    for stmt in stmt_tree(info.body) {
        // `del top[_TOP_N:]`
        if let Stmt::Delete(node) = stmt
            && node.targets.iter().any(truncation)
        {
            return true;
        }
        // `top = top[:_TOP_N]`
        if let Stmt::Assign(node) = stmt
            && let [target] = node.targets.as_slice()
            && name_of(target) == Some(name)
            && truncation_slice(node.value.as_ref(), name, &constant)
        {
            return true;
        }

        // `if len(batch) == _BATCH:`
        let mut own = Vec::new();
        stmt_own_exprs(stmt, &mut own);
        for expr in own.into_iter().flat_map(expr_tree) {
            if let Expr::Compare(node) = expr
                && let Expr::Call(call) = node.left.as_ref()
                && name_of(call.func.as_ref()) == Some("len")
                && call.args.first().and_then(name_of) == Some(name)
                && node.comparators.iter().any(&constant)
            {
                return true;
            }
        }
    }
    false
}

fn truncation_slice(value: &Expr, name: &str, constant: &dyn Fn(&Expr) -> bool) -> bool {
    let Expr::Subscript(node) = value else {
        return false;
    };
    if name_of(node.value.as_ref()) != Some(name) {
        return false;
    }
    match node.slice.as_ref() {
        Expr::Slice(slice) => {
            slice.upper.as_deref().is_some_and(constant)
                || slice.lower.as_deref().is_some_and(constant)
        }
        _ => false,
    }
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

    // `y = tuple(y)` reads the previous iteration's value; nothing invariant.
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
    let mut varying: BTreeSet<String> = info.varies.clone();
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

    // The same argument, one indirection out. A fresh list stored into a dict,
    // or handed to a helper that fills it, is depended on for its *identity*;
    // hoisting would alias every group onto one object. The build is invariant
    // in value and emphatically not in identity.
    if escapes_the_iteration(info.body, name) {
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

    let mut varying = info.varies.clone();
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
        // A slice of the object the loop is currently on copies that object,
        // not the whole input, so the loop total stays linear.
        if info.varies_with(subscript.value.as_ref()) {
            continue;
        }
        // `memoryview` slices share storage; nothing is copied.
        if is_call_of(
            subscript.value.as_ref(),
            analysis.bindings(),
            ctx.scope,
            "memoryview",
        ) {
            continue;
        }
        if !span_grows(analysis, ctx, info, subscript, slice, &varying) {
            continue;
        }

        out.push(analysis.at(
            crate::registry::LAV008,
            subscript,
            "this slice copies a span that grows with the loop; keep an index or a memoryview",
        ));
    }
}

/// `true` when the slice copies more on each pass than it did on the last.
///
/// # Moving is not growing, and this is the distinction the rule lives or dies on
///
/// `text[start:end]` over tokeniser offsets and `blob[off:off + length]` over a
/// record directory both have bounds that move with the loop, and both copy
/// each byte of the subject exactly once *in total*. What makes a slice
/// quadratic is one endpoint anchored while the other travels — a prefix that
/// keeps growing, or a tail the loop keeps re-copying. So growth needs one of:
///
/// * the loop rebinds the object to a slice of itself, consuming it; or
/// * exactly one endpoint moves, and the other is a constant or absent.
///
/// Two moving endpoints is a window, and a window tiles.
fn span_grows(
    analysis: &Analysis<'_>,
    ctx: &StmtCtx<'_>,
    info: &LoopInfo<'_>,
    subscript: &rustpython_parser::ast::ExprSubscript,
    slice: &rustpython_parser::ast::ExprSlice,
    varying: &BTreeSet<String>,
) -> bool {
    if consumes_by_reslicing(analysis, ctx, subscript) {
        return true;
    }
    if is_fixed_width(slice, analysis.source, varying) {
        return false;
    }

    let anchored = |bound: Option<&Expr>| match bound {
        None => true,
        Some(expr) => {
            integer_constant(expr, analysis.bindings(), ctx.scope, RESOLUTION_DEPTH).is_some()
                || is_negative_literal(expr)
        }
    };
    let travels = |bound: Option<&Expr>| bound.is_some_and(|expr| depends_on(expr, &info.varies));

    let lower = slice.lower.as_deref();
    let upper = slice.upper.as_deref();
    (travels(lower) && anchored(upper)) || (travels(upper) && anchored(lower))
}

/// `true` when the slice's length does not grow with the loop.
///
/// Three shapes qualify: literal bounds — `line[:19]`; a negative literal lower
/// with no upper — `line[-1:]` is one element however long `line` is; and a
/// window whose *difference* is loop-invariant — `buf[i:i + 4]`. The last is
/// how every binary-format parser is written, and missing it makes a slice rule
/// unusable on the code that slices most.
fn is_fixed_width(
    slice: &rustpython_parser::ast::ExprSlice,
    source: &str,
    varying: &BTreeSet<String>,
) -> bool {
    let lower = slice.lower.as_deref();

    let Some(upper) = slice.upper.as_deref() else {
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

/// `LAV009` — a pandas dataframe grown one row or one frame at a time.
///
/// The rule demands *positive* evidence of pandas. `x = x.append(item)` is also
/// how every persistent structure is used — a cons list, a `pyrsistent`
/// vector — where the append is O(1) and rebinding the name is the whole point.
/// Without the evidence this rule tells people that immutable data structures
/// are a performance bug.
fn lav009(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };
    let Stmt::Assign(node) = ctx.stmt else { return };
    let [target] = node.targets.as_slice() else {
        return;
    };
    let Some(name) = name_of(target) else { return };
    let Expr::Call(call) = node.value.as_ref() else {
        return;
    };
    let Expr::Attribute(func) = call.func.as_ref() else {
        return;
    };

    let grows = match func.attr.as_str() {
        "append" => {
            name_of(func.value.as_ref()) == Some(name)
                && built_by_pandas(analysis, info, ctx.scope, name)
        }
        "concat" => {
            name_of(func.value.as_ref())
                .is_some_and(|module| analysis.bindings().is_pandas_module(module))
                && concat_reuses(call, name)
        }
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

/// `true` when `name` starts life as something built by pandas.
fn built_by_pandas(analysis: &Analysis<'_>, info: &LoopInfo<'_>, scope: usize, name: &str) -> bool {
    analysis
        .initialisers_outside(info, scope, name)
        .into_iter()
        .any(|value| match value {
            Expr::Call(call) => match call.func.as_ref() {
                Expr::Attribute(func) => name_of(func.value.as_ref())
                    .is_some_and(|module| analysis.bindings().is_pandas_module(module)),
                _ => false,
            },
            _ => false,
        })
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

/// `LAV011` — a regex re-scanning a subject the loop keeps re-deriving.
fn lav011(analysis: &Analysis<'_>, ctx: &StmtCtx<'_>, out: &mut Vec<Finding>) {
    let Some(info) = analysis.hot_loop(ctx) else {
        return;
    };

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
        if !is_regex_handle(func.value.as_ref(), analysis.bindings(), ctx.scope) {
            continue;
        }

        let arguments: Vec<&Expr> = call
            .args
            .iter()
            .chain(call.keywords.iter().map(|keyword| &keyword.value))
            .collect();

        // A freshly sliced subject is a copy plus a rescan of the same tail —
        // but only when the subject outlives the iteration. A slice of *this
        // line* is scanned once and thrown away.
        let scans_a_copy = arguments.iter().any(|argument| match argument {
            Expr::Subscript(node) => {
                matches!(node.slice.as_ref(), Expr::Slice(_))
                    && !info.varies_with(node.value.as_ref())
            }
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
fn is_regex_handle(expr: &Expr, bindings: &Bindings<'_>, scope: usize) -> bool {
    name_of(expr) == Some("re") || is_call_of(expr, bindings, scope, "compile")
}
