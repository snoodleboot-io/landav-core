//! The facts every rule shares: which scope a name was bound in, which loops
//! enclose a statement, whether a loop's trip count is fixed at compile time,
//! and which values the loop varies.
//!
//! # The four predicates that decide precision
//!
//! Every rule in `F-005` reports a per-iteration cost, and each of these
//! answers a question that decides whether the cost is real:
//!
//! * **Is the trip count bounded?** `for axis in ("x", "y", "z")` runs three
//!   times whatever the input is, so the same expression that is quadratic
//!   inside `for row in rows` is a constant inside it. This has to resolve
//!   named constants — `range(_RGB_CHANNELS)` is exactly as bounded as
//!   `range(3)`, and a rule that cannot see that punishes people for naming
//!   things.
//! * **Which scope is this name from?** A module-wide name table reports
//!   `result += record.errors` as string concatenation because a *different
//!   function* has a string called `result`. Names are per-function, so the
//!   table has to be too.
//! * **Does the loop vary this value?** `cells = row.split(",")` is a fresh
//!   list per iteration, so scanning it is linear over the whole loop. The
//!   taint has to be transitive: `row` is the loop variable, `cells` comes
//!   from `row`, and anything from `cells` is per-iteration as well.
//! * **Is the object re-iterable?** Two loops over one *iterator* consume it
//!   once between them; two loops over one *list* form pairs. The headers look
//!   identical.

use std::collections::{BTreeSet, HashMap, HashSet};

use rustpython_parser::ast::{Constant, Expr, Ranged, Stmt};

use crate::syntax::{
    expr_tree, free_names, integer_literal, name_of, stmt_child_bodies, stmt_own_exprs, stmt_tree,
    target_names,
};

/// How far a type-ish question chases a name through its binding.
///
/// A binding chain is not a proof, only a heuristic, and each hop makes it
/// weaker. Four is enough for `HEADING = "..."` then `text = HEADING + "\n"`
/// and stops any cycle dead.
pub(crate) const RESOLUTION_DEPTH: u32 = 4;

/// The module scope, which every scope chain ends at.
const MODULE_SCOPE: usize = 0;

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
    /// Names whose value the loop varies, transitively from the targets.
    pub(crate) varies: BTreeSet<String>,
    /// The source text of the iterable, for comparing one loop with another.
    pub(crate) iter_text: Option<&'a str>,
    /// The body, for rules that ask what else happens in the same iteration.
    pub(crate) body: &'a [Stmt],
}

impl LoopInfo<'_> {
    /// `true` when the loop changes this expression from one pass to the next.
    ///
    /// A per-iteration object is not the loop's subject: scanning, slicing or
    /// shifting one costs its own size, and summed over the loop that is the
    /// size of the input, not its square.
    pub(crate) fn varies_with(&self, expr: &Expr) -> bool {
        free_names(expr)
            .iter()
            .any(|name| self.varies.contains(name))
    }
}

/// One statement plus the loops that enclose it and the scope it is written in.
pub(crate) struct StmtCtx<'a> {
    pub(crate) stmt: &'a Stmt,
    pub(crate) loops: Vec<usize>,
    pub(crate) scope: usize,
}

impl StmtCtx<'_> {
    /// The innermost enclosing loop, or `None` at the top level of a scope.
    pub(crate) fn innermost(&self) -> Option<usize> {
        self.loops.last().copied()
    }
}

/// What a name was bound to, when the answer is unambiguous *in its own scope*.
///
/// A name qualifies only when its scope binds it exactly once with a plain
/// assignment. Anything else — a parameter, a loop target, a rebinding, an
/// `import as` — makes the name unknown rather than guessed. A rule that
/// guesses a type is a rule that fires on somebody's `deque`.
pub(crate) struct Bindings<'a> {
    /// `scope -> enclosing scope`, ending at [`MODULE_SCOPE`].
    parents: Vec<Option<usize>>,
    single: HashMap<(usize, String), &'a Expr>,
    /// Names that reach the `pandas` module — `pd`, or `pandas` itself.
    pandas_modules: BTreeSet<String>,
}

impl<'a> Bindings<'a> {
    /// The expression a name is unambiguously bound to, searching outwards
    /// from `scope`.
    pub(crate) fn resolve(&self, scope: usize, name: &str) -> Option<&'a Expr> {
        let mut current = Some(scope);
        while let Some(index) = current {
            if let Some(found) = self.single.get(&(index, name.to_owned())) {
                return Some(found);
            }
            current = self.parents.get(index).copied().flatten();
        }
        None
    }

    /// Follows `expr` through a `Name` binding, if it is one.
    fn deref(&self, scope: usize, expr: &'a Expr) -> Option<&'a Expr> {
        name_of(expr).and_then(|name| self.resolve(scope, name))
    }

    /// `true` when `name` reaches the `pandas` module.
    pub(crate) fn is_pandas_module(&self, name: &str) -> bool {
        self.pandas_modules.contains(name)
    }
}

/// Everything one module's rules are decided from.
pub(crate) struct Program<'a> {
    pub(crate) loops: Vec<LoopInfo<'a>>,
    pub(crate) statements: Vec<StmtCtx<'a>>,
    pub(crate) bindings: Bindings<'a>,
}

/// Walks the module once, then answers the questions that need the whole file.
///
/// Three phases, because they genuinely depend on each other: the scope tree
/// and the loop structure come from the walk; the name table needs the scope
/// tree; and "is this loop bounded" needs the name table, since the bound is
/// usually a named constant.
pub(crate) fn analyse_program<'a>(module: &'a [Stmt], source: &'a str) -> Program<'a> {
    let walk = collect(module, source);
    let bindings = build_bindings(&walk);
    let loops = finish_loops(walk.loops, &bindings);
    Program {
        loops,
        statements: walk.statements,
        bindings,
    }
}

/// A loop as the walk sees it, before the name table exists.
struct PartialLoop<'a> {
    stmt: &'a Stmt,
    targets: BTreeSet<String>,
    assigned: BTreeSet<String>,
    varies: BTreeSet<String>,
    iter_text: Option<&'a str>,
    iter: Option<&'a Expr>,
    body: &'a [Stmt],
    scope: usize,
    enclosing: Vec<usize>,
}

struct Walk<'a> {
    loops: Vec<PartialLoop<'a>>,
    statements: Vec<StmtCtx<'a>>,
    parents: Vec<Option<usize>>,
    /// Parameter names introduced directly by each scope.
    parameters: Vec<Vec<String>>,
}

/// Records every loop, every statement's loop stack, and the scope tree.
///
/// Explicitly worklist-driven: a file nested a thousand `if`s deep must cost
/// heap, not stack.
fn collect<'a>(module: &'a [Stmt], source: &'a str) -> Walk<'a> {
    let mut walk = Walk {
        loops: Vec::new(),
        statements: Vec::new(),
        parents: vec![None],
        parameters: vec![Vec::new()],
    };
    let mut work: Vec<(&'a [Stmt], Vec<usize>, usize)> = vec![(module, Vec::new(), MODULE_SCOPE)];

    while let Some((body, enclosing, scope)) = work.pop() {
        for stmt in body {
            walk.statements.push(StmtCtx {
                stmt,
                loops: enclosing.clone(),
                scope,
            });

            let loop_index = match stmt {
                Stmt::For(_) | Stmt::AsyncFor(_) | Stmt::While(_) => {
                    walk.loops
                        .push(describe_loop(stmt, source, scope, &enclosing));
                    Some(walk.loops.len() - 1)
                }
                _ => None,
            };

            for (child, opens_scope) in stmt_child_bodies(stmt) {
                if child.is_empty() {
                    continue;
                }
                if opens_scope {
                    walk.parents.push(Some(scope));
                    walk.parameters.push(parameters_of(stmt));
                    let inner = walk.parents.len() - 1;
                    work.push((child, Vec::new(), inner));
                    continue;
                }
                let context = match (loop_index, is_loop_body(stmt, child)) {
                    (Some(index), true) => {
                        let mut inner = enclosing.clone();
                        inner.push(index);
                        inner
                    }
                    _ => enclosing.clone(),
                };
                work.push((child, context, scope));
            }
        }
    }

    walk
}

fn parameters_of(stmt: &Stmt) -> Vec<String> {
    let args = match stmt {
        Stmt::FunctionDef(node) => node.args.as_ref(),
        Stmt::AsyncFunctionDef(node) => node.args.as_ref(),
        _ => return Vec::new(),
    };
    let mut names: Vec<String> = args
        .posonlyargs
        .iter()
        .chain(args.args.iter())
        .chain(args.kwonlyargs.iter())
        .map(|arg| arg.def.arg.as_str().to_owned())
        .collect();
    names.extend(
        args.vararg
            .iter()
            .chain(args.kwarg.iter())
            .map(|arg| arg.arg.as_str().to_owned()),
    );
    names
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

fn describe_loop<'a>(
    stmt: &'a Stmt,
    source: &'a str,
    scope: usize,
    enclosing: &[usize],
) -> PartialLoop<'a> {
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

    PartialLoop {
        stmt,
        varies: spread_taint(body, &targets),
        targets,
        assigned: assigned_names(body),
        iter_text: iter.and_then(|iter| slice_of(source, iter)),
        iter,
        body,
        scope,
        enclosing: enclosing.to_vec(),
    }
}

/// The least set of names the loop varies, starting from its targets.
///
/// `for row in rows: cells = row.split(",")` varies `row`, therefore `cells`,
/// therefore anything derived from `cells`. Without the transitive step every
/// rule mistakes a per-iteration object for the loop's whole subject, which is
/// the single most common way a loop rule fires on linear code.
fn spread_taint(body: &[Stmt], seeds: &BTreeSet<String>) -> BTreeSet<String> {
    let mut varies = seeds.clone();
    let statements = stmt_tree(body);

    // Monotone: each pass can only add names, and there are finitely many.
    loop {
        let before = varies.len();
        for stmt in &statements {
            match stmt {
                Stmt::Assign(node) if reads_any(node.value.as_ref(), &varies) => {
                    for target in &node.targets {
                        add_target_names(target, &mut varies);
                    }
                }
                Stmt::AugAssign(node) if reads_any(node.value.as_ref(), &varies) => {
                    add_target_names(node.target.as_ref(), &mut varies);
                }
                Stmt::AnnAssign(node)
                    if node
                        .value
                        .as_deref()
                        .is_some_and(|value| reads_any(value, &varies)) =>
                {
                    add_target_names(node.target.as_ref(), &mut varies);
                }
                Stmt::For(node) if reads_any(node.iter.as_ref(), &varies) => {
                    add_target_names(node.target.as_ref(), &mut varies);
                }
                Stmt::AsyncFor(node) if reads_any(node.iter.as_ref(), &varies) => {
                    add_target_names(node.target.as_ref(), &mut varies);
                }
                Stmt::With(node) => {
                    for item in &node.items {
                        if reads_any(&item.context_expr, &varies)
                            && let Some(vars) = item.optional_vars.as_deref()
                        {
                            add_target_names(vars, &mut varies);
                        }
                    }
                }
                _ => {}
            }
        }
        if varies.len() == before {
            return varies;
        }
    }
}

fn reads_any(expr: &Expr, names: &BTreeSet<String>) -> bool {
    free_names(expr).iter().any(|name| names.contains(name))
}

fn add_target_names(target: &Expr, out: &mut BTreeSet<String>) {
    let mut names = Vec::new();
    target_names(target, &mut names);
    out.extend(names);
}

/// Builds the per-scope name table, plus the two module facts `LAV009` needs.
fn build_bindings<'a>(walk: &Walk<'a>) -> Bindings<'a> {
    let mut counts: HashMap<(usize, String), usize> = HashMap::new();
    let mut values: HashMap<(usize, String), &'a Expr> = HashMap::new();
    let mut disqualified: HashSet<(usize, String)> = HashSet::new();
    let mut pandas_modules = BTreeSet::new();

    for (scope, names) in walk.parameters.iter().enumerate() {
        for name in names {
            disqualified.insert((scope, name.clone()));
        }
    }

    for ctx in &walk.statements {
        let scope = ctx.scope;
        let ban = |target: &Expr, out: &mut HashSet<(usize, String)>| {
            let mut names = Vec::new();
            target_names(target, &mut names);
            out.extend(names.into_iter().map(|name| (scope, name)));
        };

        match ctx.stmt {
            Stmt::Assign(node) => {
                if let [target] = node.targets.as_slice()
                    && let Some(name) = name_of(target)
                {
                    *counts.entry((scope, name.to_owned())).or_default() += 1;
                    values.insert((scope, name.to_owned()), node.value.as_ref());
                } else {
                    for target in &node.targets {
                        ban(target, &mut disqualified);
                    }
                }
            }
            Stmt::AugAssign(node) => ban(node.target.as_ref(), &mut disqualified),
            Stmt::AnnAssign(node) => ban(node.target.as_ref(), &mut disqualified),
            Stmt::For(node) => ban(node.target.as_ref(), &mut disqualified),
            Stmt::AsyncFor(node) => ban(node.target.as_ref(), &mut disqualified),
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(vars) = item.optional_vars.as_deref() {
                        ban(vars, &mut disqualified);
                    }
                }
            }
            Stmt::AsyncWith(node) => {
                for item in &node.items {
                    if let Some(vars) = item.optional_vars.as_deref() {
                        ban(vars, &mut disqualified);
                    }
                }
            }
            Stmt::FunctionDef(node) => {
                disqualified.insert((scope, node.name.as_str().to_owned()));
            }
            Stmt::AsyncFunctionDef(node) => {
                disqualified.insert((scope, node.name.as_str().to_owned()));
            }
            Stmt::ClassDef(node) => {
                disqualified.insert((scope, node.name.as_str().to_owned()));
            }
            Stmt::Import(node) => {
                for alias in &node.names {
                    let bound = alias.asname.as_ref().unwrap_or(&alias.name);
                    disqualified.insert((scope, bound.as_str().to_owned()));
                    if alias.name.as_str() == "pandas" {
                        pandas_modules.insert(bound.as_str().to_owned());
                    }
                }
            }
            Stmt::ImportFrom(node) => {
                for alias in &node.names {
                    let bound = alias.asname.as_ref().unwrap_or(&alias.name);
                    disqualified.insert((scope, bound.as_str().to_owned()));
                }
                if node.module.as_ref().is_some_and(|name| name == "pandas") {
                    pandas_modules.insert("pandas".to_owned());
                }
            }
            Stmt::Global(node) => {
                disqualified.extend(
                    node.names
                        .iter()
                        .map(|name| (scope, name.as_str().to_owned())),
                );
            }
            Stmt::Nonlocal(node) => {
                disqualified.extend(
                    node.names
                        .iter()
                        .map(|name| (scope, name.as_str().to_owned())),
                );
            }
            Stmt::Try(node) => ban_handlers(&node.handlers, scope, &mut disqualified),
            Stmt::TryStar(node) => ban_handlers(&node.handlers, scope, &mut disqualified),
            _ => {}
        }
    }

    let single = values
        .into_iter()
        .filter(|(key, _)| {
            counts.get(key).copied().unwrap_or(0) == 1 && !disqualified.contains(key)
        })
        .collect();

    Bindings {
        parents: walk.parents.clone(),
        single,
        pandas_modules,
    }
}

fn ban_handlers(
    handlers: &[rustpython_parser::ast::ExceptHandler],
    scope: usize,
    out: &mut HashSet<(usize, String)>,
) {
    for handler in handlers {
        let rustpython_parser::ast::ExceptHandler::ExceptHandler(handler) = handler;
        if let Some(name) = handler.name.as_ref() {
            out.insert((scope, name.as_str().to_owned()));
        }
    }
}

/// Adds the boundedness verdict, which needed the name table to be built first.
fn finish_loops<'a>(partial: Vec<PartialLoop<'a>>, bindings: &Bindings<'a>) -> Vec<LoopInfo<'a>> {
    // An inner loop inherits the taint of every loop that encloses it: the
    // outer loop's variable is just as per-iteration from in here.
    let inherited: Vec<BTreeSet<String>> = partial
        .iter()
        .map(|info| {
            let mut varies = info.varies.clone();
            for index in &info.enclosing {
                if let Some(outer) = partial.get(*index) {
                    varies.extend(outer.varies.iter().cloned());
                }
            }
            varies
        })
        .collect();

    partial
        .into_iter()
        .zip(inherited)
        .map(|(info, varies)| LoopInfo {
            bounded: info.iter.is_some_and(|iter| {
                is_bounded_iterable(iter, bindings, info.scope, RESOLUTION_DEPTH)
            }),
            stmt: info.stmt,
            targets: info.targets,
            assigned: info.assigned,
            varies,
            iter_text: info.iter_text,
            body: info.body,
        })
        .collect()
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

/// The value of an integer that is fixed at compile time, resolving names.
///
/// `range(_RGB_CHANNELS)` is exactly as bounded as `range(3)`. A rule that only
/// accepts the literal makes naming a constant a lint failure, which is advice
/// nobody will take.
pub(crate) fn integer_constant(
    expr: &Expr,
    bindings: &Bindings<'_>,
    scope: usize,
    depth: u32,
) -> Option<u128> {
    if depth == 0 {
        return None;
    }
    if let Some(literal) = integer_literal(expr) {
        return Some(literal);
    }
    match expr {
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .and_then(|bound| integer_constant(bound, bindings, scope, depth - 1)),
        _ => None,
    }
}

/// `true` when iterating `expr` runs a number of times fixed at compile time.
fn is_bounded_iterable(expr: &Expr, bindings: &Bindings<'_>, scope: usize, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::Tuple(_) | Expr::List(_) | Expr::Set(_) | Expr::Dict(_) => true,
        Expr::Constant(node) => matches!(node.value, Constant::Str(_) | Constant::Bytes(_)),
        Expr::Call(node) => match node.func.as_ref() {
            Expr::Name(func) => match func.id.as_str() {
                "range" => {
                    !node.args.is_empty()
                        && node.args.iter().all(|arg| {
                            integer_constant(arg, bindings, scope, RESOLUTION_DEPTH).is_some()
                        })
                }
                // A view of a fixed mapping is as fixed as the mapping.
                "enumerate" | "reversed" | "sorted" | "list" | "tuple" | "set" | "frozenset" => {
                    node.args
                        .first()
                        .is_some_and(|arg| is_bounded_iterable(arg, bindings, scope, depth - 1))
                }
                "zip" => {
                    !node.args.is_empty()
                        && node
                            .args
                            .iter()
                            .any(|arg| is_bounded_iterable(arg, bindings, scope, depth - 1))
                }
                _ => false,
            },
            // `_SUBSTITUTIONS.items()` over a dict literal is four iterations.
            Expr::Attribute(func) => {
                matches!(func.attr.as_str(), "items" | "keys" | "values")
                    && node.args.is_empty()
                    && is_dict_like(func.value.as_ref(), bindings, scope, depth - 1)
            }
            _ => false,
        },
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_bounded_iterable(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

fn is_dict_like(expr: &Expr, bindings: &Bindings<'_>, scope: usize, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::Dict(_) => true,
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_dict_like(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

/// `true` when `expr` names a one-shot iterator rather than a re-iterable
/// collection.
///
/// Two nested loops over an *iterator* share one left-to-right pass; two over a
/// *list* form every pair. The loop headers are identical, so this is the only
/// thing that tells them apart.
pub(crate) fn is_one_shot_iterator(
    expr: &Expr,
    bindings: &Bindings<'_>,
    scope: usize,
    depth: u32,
) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::GeneratorExp(_) => true,
        Expr::Call(node) => matches!(
            name_of(node.func.as_ref()),
            Some("iter" | "enumerate" | "zip" | "reversed" | "map" | "filter")
        ),
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_one_shot_iterator(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

/// `true` when `expr` is, as far as the binding chain shows, a `str`.
pub(crate) fn is_str_expr(expr: &Expr, bindings: &Bindings<'_>, scope: usize, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::Constant(node) => matches!(node.value, Constant::Str(_)),
        Expr::JoinedStr(_) => true,
        Expr::BinOp(node) => {
            matches!(node.op, rustpython_parser::ast::Operator::Add)
                && (is_str_expr(node.left.as_ref(), bindings, scope, depth - 1)
                    || is_str_expr(node.right.as_ref(), bindings, scope, depth - 1))
        }
        Expr::IfExp(node) => {
            is_str_expr(node.body.as_ref(), bindings, scope, depth - 1)
                || is_str_expr(node.orelse.as_ref(), bindings, scope, depth - 1)
        }
        Expr::Call(node) => match node.func.as_ref() {
            Expr::Name(func) => matches!(func.id.as_str(), "str" | "repr" | "format"),
            Expr::Attribute(func) => {
                func.attr.as_str() == "join"
                    && is_str_expr(func.value.as_ref(), bindings, scope, depth - 1)
            }
            _ => false,
        },
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_str_expr(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

/// The smallest list literal `LAV002` treats as worth a set.
///
/// Below this a scan of interned constants beats hashing the probe: eight
/// elements average four pointer comparisons, which is roughly where a hash
/// plus a probe starts to win. Reporting `method in ["GET", "HEAD", "OPTIONS",
/// "TRACE"]` would be advice to make the code slower.
const MIN_SCANNED_LIST: usize = 8;

/// `true` when membership against `expr` is a linear scan worth replacing.
pub(crate) fn is_scanned_list(
    expr: &Expr,
    bindings: &Bindings<'_>,
    scope: usize,
    depth: u32,
) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::List(node) => {
            node.elts.len() >= MIN_SCANNED_LIST && !node.elts.iter().any(is_unhashable_literal)
        }
        Expr::ListComp(_) => true,
        Expr::Call(node) => {
            matches!(name_of(node.func.as_ref()), Some("list" | "sorted")) && !node.args.is_empty()
        }
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_scanned_list(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

/// `true` when `expr` is a short, fixed sequence written out in the source.
///
/// A seven-entry weekday table is a bounded scan however hot the loop is, and
/// there is no position map worth building for it.
pub(crate) fn is_short_constant_sequence(
    expr: &Expr,
    bindings: &Bindings<'_>,
    scope: usize,
    depth: u32,
) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expr::List(node) => node.elts.len() < MIN_SCANNED_LIST,
        Expr::Tuple(node) => node.elts.len() < MIN_SCANNED_LIST,
        Expr::Name(_) => bindings
            .deref(scope, expr)
            .is_some_and(|bound| is_short_constant_sequence(bound, bindings, scope, depth - 1)),
        _ => false,
    }
}

/// `true` for an element that cannot go in a `set` at all.
///
/// A list of dicts has no set to move to, so the rewrite the rule would be
/// recommending does not exist.
fn is_unhashable_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Dict(_) | Expr::List(_) | Expr::Set(_) | Expr::DictComp(_) | Expr::ListComp(_)
    )
}

/// `true` when `expr` is a name bound to a call of `constructor`.
pub(crate) fn is_call_of(
    expr: &Expr,
    bindings: &Bindings<'_>,
    scope: usize,
    constructor: &str,
) -> bool {
    let Some(bound) = bindings.deref(scope, expr) else {
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

/// The names an expression exposes *directly*, without going through a
/// subscript, an attribute or a call.
///
/// `f(xs)` and `f((a, xs))` hand `xs` itself to the callee, which can keep or
/// mutate it. `f(xs[0])` hands over an element and says nothing about `xs`.
/// Several rules turn on that difference, because it is the difference between
/// a value and an identity.
pub(crate) fn surface_names(expr: &Expr, out: &mut Vec<String>) {
    let mut stack = vec![expr];
    while let Some(current) = stack.pop() {
        match current {
            Expr::Name(node) => out.push(node.id.as_str().to_owned()),
            Expr::Tuple(node) => stack.extend(node.elts.iter()),
            Expr::List(node) => stack.extend(node.elts.iter()),
            Expr::Set(node) => stack.extend(node.elts.iter()),
            Expr::Starred(node) => stack.push(node.value.as_ref()),
            _ => {}
        }
    }
}

/// `true` when the loop hands `name` itself to something that can keep it.
///
/// A fresh collection stored into another structure, or passed to a helper that
/// fills it, has an *identity* the loop depends on. Hoisting it would alias
/// every iteration onto one object, which changes the answer rather than the
/// cost — the difference between "built too often" and "built once per group
/// on purpose".
pub(crate) fn escapes_the_iteration(body: &[Stmt], name: &str) -> bool {
    for stmt in stmt_tree(body) {
        match stmt {
            // `groups[label] = members` keeps the object, not a copy of it.
            Stmt::Assign(node)
                if node
                    .targets
                    .iter()
                    .any(|target| !matches!(target, Expr::Name(_)))
                    && carries(node.value.as_ref(), name) =>
            {
                return true;
            }
            Stmt::Return(node)
                if node
                    .value
                    .as_deref()
                    .is_some_and(|value| carries(value, name)) =>
            {
                return true;
            }
            _ => {}
        }

        let mut own = Vec::new();
        stmt_own_exprs(stmt, &mut own);
        for expr in own.into_iter().flat_map(expr_tree) {
            match expr {
                Expr::Call(call) => {
                    let handed_over = call
                        .args
                        .iter()
                        .chain(call.keywords.iter().map(|keyword| &keyword.value))
                        .any(|argument| carries(argument, name));
                    if handed_over {
                        return true;
                    }
                }
                Expr::Yield(node)
                    if node
                        .value
                        .as_deref()
                        .is_some_and(|value| carries(value, name)) =>
                {
                    return true;
                }
                _ => {}
            }
        }
    }

    false
}

/// `true` when `expr` hands `name` over directly, rather than an element of it.
fn carries(expr: &Expr, name: &str) -> bool {
    let mut surfaces = Vec::new();
    surface_names(expr, &mut surfaces);
    surfaces.iter().any(|found| found == name)
}
