//! The facts every rule shares: which loops enclose a statement, whether a
//! loop's trip count is fixed at compile time, and what a bare name was bound
//! to.
//!
//! # Why "is this loop bounded?" is the single most valuable predicate here
//!
//! Every rule in `F-005` reports a per-iteration cost. `for axis in ("x", "y",
//! "z")` runs three times whatever the input is, so the same expression that is
//! quadratic inside `for row in rows` is a constant inside it. Without this
//! predicate the whole rule set fires on idiomatic three-element loops, which
//! is exactly the "switched off within a week" failure the negative corpus is
//! built to catch — four of the forty negative fixtures are nothing but this.

use std::collections::{BTreeSet, HashMap, HashSet};

use rustpython_parser::ast::{Expr, Ranged, Stmt};

use crate::syntax::{expr_tree, free_names, name_of, stmt_child_bodies, stmt_tree, target_names};

/// How far a type-ish question chases a name through its binding.
///
/// A binding chain is not a proof, only a heuristic, and each hop makes it
/// weaker. Four is enough for `HEADING = "..."` then `text = HEADING + "\n"`
/// and stops any cycle dead.
const RESOLUTION_DEPTH: u32 = 4;

/// One `for` or `while` statement, with what the rules need to know about it.
pub(crate) struct LoopInfo<'a> {
    /// The `for`/`while` statement itself.
    pub(crate) stmt: &'a Stmt,
    /// `true` when the trip count is fixed at compile time.
    pub(crate) bounded: bool,
    /// Names bound by the loop header — empty for `while`.
    pub(crate) targets: BTreeSet<String>,
    /// Names assigned anywhere in the body, at any depth.
    pub(crate) assigned: BTreeSet<String>,
    /// The source text of the iterable, for comparing one loop with another.
    pub(crate) iter_text: Option<&'a str>,
    /// The body, for rules that ask what else happens in the same iteration.
    pub(crate) body: &'a [Stmt],
}

/// One statement plus the loops that enclose it, outermost first.
pub(crate) struct StmtCtx<'a> {
    pub(crate) stmt: &'a Stmt,
    pub(crate) loops: Vec<usize>,
}

impl StmtCtx<'_> {
    /// The innermost enclosing loop, or `None` at the top level of a scope.
    pub(crate) fn innermost(&self) -> Option<usize> {
        self.loops.last().copied()
    }
}

/// What a name was bound to, when the answer is unambiguous.
///
/// A name qualifies only when the whole file binds it exactly once with a plain
/// assignment. Anything else — a parameter, a loop target, a rebinding, an
/// `import as` — makes the name unknown rather than guessed. A rule that guesses
/// a type is a rule that fires on somebody's `deque`.
pub(crate) struct Bindings<'a> {
    single: HashMap<String, &'a Expr>,
}

impl<'a> Bindings<'a> {
    pub(crate) fn new(module: &'a [Stmt]) -> Self {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut values: HashMap<String, &'a Expr> = HashMap::new();
        let mut disqualified: HashSet<String> = HashSet::new();

        for stmt in stmt_tree(module) {
            match stmt {
                Stmt::Assign(node) => {
                    if let [target] = node.targets.as_slice()
                        && let Some(name) = name_of(target)
                    {
                        *counts.entry(name.to_owned()).or_default() += 1;
                        values.insert(name.to_owned(), node.value.as_ref());
                    } else {
                        for target in &node.targets {
                            disqualify(target, &mut disqualified);
                        }
                    }
                }
                Stmt::AugAssign(node) => disqualify(&node.target, &mut disqualified),
                Stmt::AnnAssign(node) => disqualify(&node.target, &mut disqualified),
                Stmt::For(node) => disqualify(&node.target, &mut disqualified),
                Stmt::AsyncFor(node) => disqualify(&node.target, &mut disqualified),
                Stmt::With(node) => disqualify_with(&node.items, &mut disqualified),
                Stmt::AsyncWith(node) => disqualify_with(&node.items, &mut disqualified),
                Stmt::FunctionDef(node) => {
                    disqualified.insert(node.name.as_str().to_owned());
                    disqualify_params(&node.args, &mut disqualified);
                }
                Stmt::AsyncFunctionDef(node) => {
                    disqualified.insert(node.name.as_str().to_owned());
                    disqualify_params(&node.args, &mut disqualified);
                }
                Stmt::ClassDef(node) => {
                    disqualified.insert(node.name.as_str().to_owned());
                }
                Stmt::Import(node) => disqualify_aliases(&node.names, &mut disqualified),
                Stmt::ImportFrom(node) => disqualify_aliases(&node.names, &mut disqualified),
                Stmt::Global(node) => {
                    disqualified.extend(node.names.iter().map(|name| name.as_str().to_owned()));
                }
                Stmt::Nonlocal(node) => {
                    disqualified.extend(node.names.iter().map(|name| name.as_str().to_owned()));
                }
                Stmt::Try(node) => disqualify_handlers(&node.handlers, &mut disqualified),
                Stmt::TryStar(node) => disqualify_handlers(&node.handlers, &mut disqualified),
                _ => {}
            }
        }

        let single = values
            .into_iter()
            .filter(|(name, _)| {
                counts.get(name).copied().unwrap_or(0) == 1 && !disqualified.contains(name)
            })
            .collect();

        Self { single }
    }

    /// The expression a name is unambiguously bound to.
    pub(crate) fn resolve(&self, name: &str) -> Option<&'a Expr> {
        self.single.get(name).copied()
    }

    /// Follows `expr` through a `Name` binding, if it is one.
    fn deref(&self, expr: &'a Expr) -> Option<&'a Expr> {
        name_of(expr).and_then(|name| self.resolve(name))
    }
}

fn disqualify(target: &Expr, out: &mut HashSet<String>) {
    let mut names = Vec::new();
    target_names(target, &mut names);
    out.extend(names);
}

fn disqualify_with(items: &[rustpython_parser::ast::WithItem], out: &mut HashSet<String>) {
    for item in items {
        if let Some(vars) = item.optional_vars.as_deref() {
            disqualify(vars, out);
        }
    }
}

fn disqualify_handlers(
    handlers: &[rustpython_parser::ast::ExceptHandler],
    out: &mut HashSet<String>,
) {
    for handler in handlers {
        let rustpython_parser::ast::ExceptHandler::ExceptHandler(handler) = handler;
        if let Some(name) = handler.name.as_ref() {
            out.insert(name.as_str().to_owned());
        }
    }
}

fn disqualify_aliases(names: &[rustpython_parser::ast::Alias], out: &mut HashSet<String>) {
    for alias in names {
        let bound = alias.asname.as_ref().unwrap_or(&alias.name);
        out.insert(bound.as_str().to_owned());
    }
}

fn disqualify_params(args: &rustpython_parser::ast::Arguments, out: &mut HashSet<String>) {
    for arg in args
        .posonlyargs
        .iter()
        .chain(args.args.iter())
        .chain(args.kwonlyargs.iter())
    {
        out.insert(arg.def.arg.as_str().to_owned());
    }
    for arg in args.vararg.iter().chain(args.kwarg.iter()) {
        out.insert(arg.arg.as_str().to_owned());
    }
}

/// Walks the module, recording every loop and the loop stack each statement
/// sits under.
///
/// Explicitly worklist-driven: a file nested a thousand `if`s deep must cost
/// heap, not stack.
pub(crate) fn collect<'a>(
    module: &'a [Stmt],
    source: &'a str,
    bindings: &Bindings<'a>,
) -> (Vec<LoopInfo<'a>>, Vec<StmtCtx<'a>>) {
    let mut loops: Vec<LoopInfo<'a>> = Vec::new();
    let mut statements: Vec<StmtCtx<'a>> = Vec::new();
    let mut work: Vec<(&'a [Stmt], Vec<usize>)> = vec![(module, Vec::new())];

    while let Some((body, enclosing)) = work.pop() {
        for stmt in body {
            statements.push(StmtCtx {
                stmt,
                loops: enclosing.clone(),
            });

            let loop_index = match stmt {
                Stmt::For(_) | Stmt::AsyncFor(_) | Stmt::While(_) => {
                    loops.push(describe_loop(stmt, source, bindings));
                    Some(loops.len() - 1)
                }
                _ => None,
            };

            for (child, new_scope) in stmt_child_bodies(stmt) {
                if child.is_empty() {
                    continue;
                }
                let context = if new_scope {
                    Vec::new()
                } else {
                    match (loop_index, is_loop_body(stmt, child)) {
                        (Some(index), true) => {
                            let mut inner = enclosing.clone();
                            inner.push(index);
                            inner
                        }
                        _ => enclosing.clone(),
                    }
                };
                work.push((child, context));
            }
        }
    }

    (loops, statements)
}

/// `true` when `child` is the repeated body of `stmt` rather than its `else`.
fn is_loop_body(stmt: &Stmt, child: &[Stmt]) -> bool {
    let body = match stmt {
        Stmt::For(node) => node.body.as_slice(),
        Stmt::AsyncFor(node) => node.body.as_slice(),
        Stmt::While(node) => node.body.as_slice(),
        _ => return false,
    };
    std::ptr::eq(body.as_ptr(), child.as_ptr())
}

fn describe_loop<'a>(stmt: &'a Stmt, source: &'a str, bindings: &Bindings<'a>) -> LoopInfo<'a> {
    let (target, iter, body) = match stmt {
        Stmt::For(node) => (
            Some(node.target.as_ref()),
            Some(node.iter.as_ref()),
            node.body.as_slice(),
        ),
        Stmt::AsyncFor(node) => (
            Some(node.target.as_ref()),
            Some(node.iter.as_ref()),
            node.body.as_slice(),
        ),
        Stmt::While(node) => (None, None, node.body.as_slice()),
        _ => (None, None, [].as_slice()),
    };

    let mut targets = BTreeSet::new();
    if let Some(target) = target {
        let mut names = Vec::new();
        target_names(target, &mut names);
        targets.extend(names);
    }

    LoopInfo {
        stmt,
        bounded: iter.is_some_and(|iter| is_bounded_iterable(iter, bindings, RESOLUTION_DEPTH)),
        targets,
        assigned: assigned_names(body),
        iter_text: iter.and_then(|iter| slice_of(source, iter)),
        body,
    }
}

/// Every name the statements bind, at any depth below them.
pub(crate) fn assigned_names(body: &[Stmt]) -> BTreeSet<String> {
    let mut names = Vec::new();
    for stmt in stmt_tree(body) {
        match stmt {
            Stmt::Assign(node) => {
                for target in &node.targets {
                    target_names(target, &mut names);
                }
            }
            Stmt::AugAssign(node) => target_names(&node.target, &mut names),
            Stmt::AnnAssign(node) => target_names(&node.target, &mut names),
            Stmt::For(node) => target_names(&node.target, &mut names),
            Stmt::AsyncFor(node) => target_names(&node.target, &mut names),
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(vars) = item.optional_vars.as_deref() {
                        target_names(vars, &mut names);
                    }
                }
            }
            _ => {}
        }
    }
    names.into_iter().collect()
}

/// The source text a node spans, or `None` if its range is not a char boundary.
pub(crate) fn slice_of<'a, T: Ranged>(source: &'a str, node: &T) -> Option<&'a str> {
    let range = node.range();
    source.get(range.start().to_usize()..range.end().to_usize())
}

/// `true` when iterating `expr` runs a number of times fixed at compile time.
fn is_bounded_iterable(expr: &Expr, bindings: &Bindings<'_>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::Tuple(_) | Expr::List(_) | Expr::Set(_) | Expr::Dict(_) => true,
        Expr::Constant(node) => matches!(
            node.value,
            rustpython_parser::ast::Constant::Str(_) | rustpython_parser::ast::Constant::Bytes(_)
        ),
        Expr::Call(node) => {
            name_of(node.func.as_ref()) == Some("range")
                && !node.args.is_empty()
                && node
                    .args
                    .iter()
                    .all(|arg| crate::syntax::integer_literal(arg).is_some())
        }
        Expr::Name(_) => bindings
            .deref(expr)
            .is_some_and(|bound| is_bounded_iterable(bound, bindings, depth - 1)),
        _ => false,
    }
}

/// `true` when `expr` is, as far as the binding chain shows, a `str`.
pub(crate) fn is_str_expr(expr: &Expr, bindings: &Bindings<'_>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::Constant(node) => matches!(node.value, rustpython_parser::ast::Constant::Str(_)),
        Expr::JoinedStr(_) => true,
        Expr::BinOp(node) => {
            matches!(node.op, rustpython_parser::ast::Operator::Add)
                && (is_str_expr(node.left.as_ref(), bindings, depth - 1)
                    || is_str_expr(node.right.as_ref(), bindings, depth - 1))
        }
        Expr::IfExp(node) => {
            is_str_expr(node.body.as_ref(), bindings, depth - 1)
                || is_str_expr(node.orelse.as_ref(), bindings, depth - 1)
        }
        Expr::Call(node) => match node.func.as_ref() {
            Expr::Name(func) => matches!(func.id.as_str(), "str" | "repr" | "format"),
            Expr::Attribute(func) => {
                func.attr.as_str() == "join"
                    && is_str_expr(func.value.as_ref(), bindings, depth - 1)
            }
            _ => false,
        },
        Expr::Name(_) => bindings
            .deref(expr)
            .is_some_and(|bound| is_str_expr(bound, bindings, depth - 1)),
        _ => false,
    }
}

/// The smallest list literal `LAV002` treats as worth a set.
///
/// Two or three comparisons beat a hash and an allocation, so a short literal
/// is not a defect — `row.status in ("ok", "warn")` is the fastest spelling
/// there is, and rewriting it would be a pessimisation.
const MIN_SCANNED_LIST: usize = 4;

/// `true` when membership against `expr` is a linear scan of a list.
pub(crate) fn is_scanned_list(expr: &Expr, bindings: &Bindings<'_>, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::List(node) => node.elts.len() >= MIN_SCANNED_LIST,
        Expr::ListComp(_) => true,
        Expr::Call(node) => {
            matches!(name_of(node.func.as_ref()), Some("list" | "sorted")) && !node.args.is_empty()
        }
        Expr::Name(_) => bindings
            .deref(expr)
            .is_some_and(|bound| is_scanned_list(bound, bindings, depth - 1)),
        _ => false,
    }
}

/// `true` when `expr` is a name bound to a call of `constructor`.
pub(crate) fn is_call_of(expr: &Expr, bindings: &Bindings<'_>, constructor: &str) -> bool {
    let Some(bound) = bindings.deref(expr) else {
        return false;
    };
    let Expr::Call(call) = bound else {
        return false;
    };
    match call.func.as_ref() {
        Expr::Name(func) => func.id.as_str() == constructor,
        Expr::Attribute(func) => func.attr.as_str() == constructor,
        _ => false,
    }
}

/// `true` when any expression under `expr` reads one of `names`.
pub(crate) fn depends_on(expr: &Expr, names: &BTreeSet<String>) -> bool {
    free_names(expr).iter().any(|name| names.contains(name))
}

/// `true` when `inner` lies inside `outer`'s source range.
pub(crate) fn contains<A: Ranged, B: Ranged>(outer: &A, inner: &B) -> bool {
    let outer = outer.range();
    let inner = inner.range();
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

/// `true` when these statements *read* through a subscript.
///
/// `LAV010`'s narrowing turns on it: `d[k]` inside a `try` has a total
/// alternative in `d.get(k)`, whereas `open(path)` has none. The read has to be
/// a load — `d[k] = v` can raise the same `KeyError` but `get` does not express
/// a store, so the rule would be giving advice that does not apply.
pub(crate) fn body_reads_a_subscript(body: &[Stmt]) -> bool {
    let mut exprs = Vec::new();
    for stmt in stmt_tree(body) {
        crate::syntax::stmt_own_exprs(stmt, &mut exprs);
    }
    exprs
        .into_iter()
        .flat_map(expr_tree)
        .any(|expr| match expr {
            Expr::Subscript(node) => matches!(node.ctx, rustpython_parser::ast::ExprContext::Load),
            _ => false,
        })
}
