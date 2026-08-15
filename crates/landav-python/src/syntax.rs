//! Syntax plumbing shared by every pattern rule.
//!
//! Three jobs, none of which is a rule:
//!
//! * turning a byte offset into the `(line, column)` pair [`crate::Location`]
//!   promises — 1-based line, 1-based **UTF-8 byte** column within that line;
//! * enumerating the children of an AST node, so that traversal can be driven
//!   by an explicit worklist rather than by the call stack (non-negotiable 2:
//!   a deeply nested expression must not blow the stack);
//! * a depth guard applied to the *bytes* before the parser ever sees them,
//!   because the parser itself is recursive and we do not get to choose how it
//!   fails.

use rustpython_parser::ast::{Expr, ExprContext, Stmt};

/// The deepest block nesting the frontend will parse.
///
/// Real Python does not approach this; generated or hostile input does, and a
/// stack overflow is an abort, not an error a caller can act on. Rejecting
/// first turns "the process died" into a [`crate::PythonError::Parse`] with a
/// position on it.
pub(crate) const MAX_NESTING_DEPTH: usize = 120;

/// The deepest single expression the frontend will parse.
///
/// Bracket depth alone does not bound stack use, which is the trap:
/// `x = 1 + 1 + …` and `x = ---…1` contain no brackets at all and still build
/// a spine one node deep per operator. Half a million of either overflows the
/// stack while parsing *or* while dropping the tree, and an abort loses the
/// blame path that makes a partial result useful.
///
/// The bound is on the operators on one path through an expression, so a flat
/// literal — `[-1, -1, -1, …]` for a hundred thousand elements — is *not*
/// rejected: a comma ends one operand and starts the next, so the spine there
/// is two deep however long the list is. That distinction is the whole reason
/// this is not simply a cap on file size.
pub(crate) const MAX_EXPRESSION_DEPTH: usize = 10_000;

/// Maps byte offsets to 1-based line and 1-based byte column.
pub(crate) struct LineIndex {
    /// Byte offset of the first byte of each line.
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(source: &str) -> Self {
        let mut starts = vec![0_usize];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(offset + 1);
            }
        }
        Self { starts }
    }

    /// `(line, column)`, both 1-based; the column counts UTF-8 bytes.
    pub(crate) fn position(&self, offset: usize) -> (u32, u32) {
        let line_index = match self.starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insertion) => insertion.saturating_sub(1),
        };
        let start = self.starts.get(line_index).copied().unwrap_or(0);
        let line = clamp_u32(line_index + 1);
        let column = clamp_u32(offset.saturating_sub(start) + 1);
        (line, column)
    }
}

/// `usize` to `u32` without an `as` cast; the workspace denies truncation.
fn clamp_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Where the nesting guard tripped, as a byte offset, or `None` if the source
/// is within the depth the parser can be trusted with.
///
/// Brackets inside string literals and comments do not count, so a docstring
/// full of `[[[[` is not mistaken for a deeply nested expression.
pub(crate) fn nesting_overflow(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 0_usize;
    let mut indents = vec![0_usize];
    let mut at_line_start = true;
    // One operator count per open bracket, so the guard measures the operators
    // on the current path rather than in the file.
    let mut chains = vec![0_usize];
    let mut continued = false;

    while index < bytes.len() {
        let byte = bytes[index];
        let bracket_depth = chains.len() - 1;

        if at_line_start && bracket_depth == 0 {
            let (indent, next) = measure_indent(bytes, index);
            index = next;
            at_line_start = false;
            if !continued {
                chains = vec![0_usize];
            }
            continued = false;
            if next < bytes.len()
                && bytes[next] != b'\n'
                && bytes[next] != b'#'
                && let Some(overflow) = push_indent(&mut indents, indent, next)
            {
                return Some(overflow);
            }
            continue;
        }

        match byte {
            b'\n' => {
                at_line_start = true;
                index += 1;
            }
            b'#' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'"' | b'\'' => index = skip_string(bytes, index),
            b'\\' => {
                continued = true;
                index += 1;
            }
            b'(' | b'[' | b'{' => {
                chains.push(0);
                if bracket_depth + 1 > MAX_NESTING_DEPTH {
                    return Some(index);
                }
                index += 1;
            }
            b')' | b']' | b'}' => {
                if chains.len() > 1 {
                    chains.pop();
                }
                index += 1;
            }
            // A separator ends the operand, so the spine restarts here.
            b',' | b';' | b':' => {
                if let Some(chain) = chains.last_mut() {
                    *chain = 0;
                }
                index += 1;
            }
            b'+' | b'-' | b'*' | b'/' | b'%' | b'@' | b'&' | b'|' | b'^' | b'~' | b'<' | b'>'
            | b'.' => {
                if let Some(overflow) = deepen(&mut chains, 1, index) {
                    return Some(overflow);
                }
                index += 1;
            }
            _ => {
                let word = word_at(bytes, index);
                if matches!(word, b"not" | b"await" | b"lambda")
                    && let Some(overflow) = deepen(&mut chains, 1, index)
                {
                    return Some(overflow);
                }
                index += word.len().max(1);
            }
        }
    }

    None
}

/// The identifier-shaped run starting at `index`, or an empty slice.
fn word_at(bytes: &[u8], index: usize) -> &[u8] {
    if !bytes[index].is_ascii_alphabetic() && bytes[index] != b'_' {
        return &[];
    }
    if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
        // Mid-identifier: `notes` must not read as `not`.
        return &[];
    }
    let mut end = index;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    &bytes[index..end]
}

/// Adds to the innermost operator chain, reporting the offset at which the
/// total spine — operators plus the brackets containing them — grew past
/// [`MAX_EXPRESSION_DEPTH`].
fn deepen(chains: &mut [usize], by: usize, offset: usize) -> Option<usize> {
    if let Some(chain) = chains.last_mut() {
        *chain += by;
    }
    let total = chains.iter().sum::<usize>() + chains.len() - 1;
    (total > MAX_EXPRESSION_DEPTH).then_some(offset)
}

/// Column width of the leading whitespace at `index`, and the offset just past
/// it. Tabs count as one, which is enough for a depth guard.
fn measure_indent(bytes: &[u8], index: usize) -> (usize, usize) {
    let mut cursor = index;
    let mut width = 0_usize;
    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        width += 1;
        cursor += 1;
    }
    (width, cursor)
}

/// Maintains the indentation stack, returning the offset at which the block
/// nesting became deeper than [`MAX_NESTING_DEPTH`].
fn push_indent(indents: &mut Vec<usize>, indent: usize, offset: usize) -> Option<usize> {
    while indents.len() > 1 && indents.last().is_some_and(|top| *top > indent) {
        indents.pop();
    }
    if indents.last().is_some_and(|top| *top < indent) {
        indents.push(indent);
        if indents.len() > MAX_NESTING_DEPTH {
            return Some(offset);
        }
    }
    None
}

/// Offset just past the string literal starting at `index`.
///
/// Shared with [`crate::noqa`], which has the same question to answer for the
/// opposite reason: the depth guard must not count a bracket inside a literal,
/// and the suppression scanner must not read a `#` inside one as a comment.
/// Two scanners disagreeing about where a literal ends is exactly how
/// `SEPARATOR = "# noqa: LAV003"` would become a waiver.
pub(crate) fn skip_string(bytes: &[u8], index: usize) -> usize {
    let quote = bytes[index];
    let triple = bytes.get(index + 1) == Some(&quote) && bytes.get(index + 2) == Some(&quote);
    let width = if triple { 3 } else { 1 };
    let mut cursor = index + width;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'\n' if !triple => return cursor,
            byte if byte == quote => {
                if !triple {
                    return cursor + 1;
                }
                if bytes.get(cursor + 1) == Some(&quote) && bytes.get(cursor + 2) == Some(&quote) {
                    return cursor + 3;
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }

    cursor
}

/// Pushes every direct sub-expression of `expr` onto `out`.
///
/// Deliberately total over the enum rather than falling back to a wildcard: a
/// new node kind in a future parser release should fail the build, not silently
/// hide a subtree from every rule.
pub(crate) fn expr_children<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    match expr {
        Expr::BoolOp(node) => out.extend(node.values.iter()),
        Expr::NamedExpr(node) => {
            out.push(node.target.as_ref());
            out.push(node.value.as_ref());
        }
        Expr::BinOp(node) => {
            out.push(node.left.as_ref());
            out.push(node.right.as_ref());
        }
        Expr::UnaryOp(node) => out.push(node.operand.as_ref()),
        Expr::Lambda(node) => out.push(node.body.as_ref()),
        Expr::IfExp(node) => {
            out.push(node.test.as_ref());
            out.push(node.body.as_ref());
            out.push(node.orelse.as_ref());
        }
        Expr::Dict(node) => {
            out.extend(node.keys.iter().flatten());
            out.extend(node.values.iter());
        }
        Expr::Set(node) => out.extend(node.elts.iter()),
        Expr::ListComp(node) => {
            out.push(node.elt.as_ref());
            push_generators(&node.generators, out);
        }
        Expr::SetComp(node) => {
            out.push(node.elt.as_ref());
            push_generators(&node.generators, out);
        }
        Expr::DictComp(node) => {
            out.push(node.key.as_ref());
            out.push(node.value.as_ref());
            push_generators(&node.generators, out);
        }
        Expr::GeneratorExp(node) => {
            out.push(node.elt.as_ref());
            push_generators(&node.generators, out);
        }
        Expr::Await(node) => out.push(node.value.as_ref()),
        Expr::Yield(node) => out.extend(node.value.as_deref()),
        Expr::YieldFrom(node) => out.push(node.value.as_ref()),
        Expr::Compare(node) => {
            out.push(node.left.as_ref());
            out.extend(node.comparators.iter());
        }
        Expr::Call(node) => {
            out.push(node.func.as_ref());
            out.extend(node.args.iter());
            out.extend(node.keywords.iter().map(|keyword| &keyword.value));
        }
        Expr::FormattedValue(node) => {
            out.push(node.value.as_ref());
            out.extend(node.format_spec.as_deref());
        }
        Expr::JoinedStr(node) => out.extend(node.values.iter()),
        Expr::Constant(_) | Expr::Name(_) => {}
        Expr::Attribute(node) => out.push(node.value.as_ref()),
        Expr::Subscript(node) => {
            out.push(node.value.as_ref());
            out.push(node.slice.as_ref());
        }
        Expr::Starred(node) => out.push(node.value.as_ref()),
        Expr::List(node) => out.extend(node.elts.iter()),
        Expr::Tuple(node) => out.extend(node.elts.iter()),
        Expr::Slice(node) => {
            out.extend(node.lower.as_deref());
            out.extend(node.upper.as_deref());
            out.extend(node.step.as_deref());
        }
    }
}

fn push_generators<'a>(
    generators: &'a [rustpython_parser::ast::Comprehension],
    out: &mut Vec<&'a Expr>,
) {
    for generator in generators {
        out.push(&generator.target);
        out.push(&generator.iter);
        out.extend(generator.ifs.iter());
    }
}

/// Every expression in the tree rooted at `root`, `root` included.
///
/// Worklist rather than recursion, so depth costs heap and not stack.
pub(crate) fn expr_tree(root: &Expr) -> Vec<&Expr> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    let mut children = Vec::new();
    while let Some(current) = stack.pop() {
        found.push(current);
        children.clear();
        expr_children(current, &mut children);
        stack.extend(children.iter().copied());
    }
    found
}

/// The expressions a statement evaluates *itself*, excluding anything inside a
/// nested statement body.
///
/// Keeping the two apart is what lets the driver visit every expression exactly
/// once while still knowing which loops enclose it.
pub(crate) fn stmt_own_exprs<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Expr>) {
    match stmt {
        Stmt::FunctionDef(node) => {
            out.extend(node.decorator_list.iter());
            out.extend(node.returns.as_deref());
        }
        Stmt::AsyncFunctionDef(node) => {
            out.extend(node.decorator_list.iter());
            out.extend(node.returns.as_deref());
        }
        Stmt::ClassDef(node) => {
            out.extend(node.bases.iter());
            out.extend(node.keywords.iter().map(|keyword| &keyword.value));
            out.extend(node.decorator_list.iter());
        }
        Stmt::Return(node) => out.extend(node.value.as_deref()),
        Stmt::Delete(node) => out.extend(node.targets.iter()),
        Stmt::Assign(node) => {
            out.extend(node.targets.iter());
            out.push(node.value.as_ref());
        }
        Stmt::TypeAlias(node) => {
            out.push(node.name.as_ref());
            out.push(node.value.as_ref());
        }
        Stmt::AugAssign(node) => {
            out.push(node.target.as_ref());
            out.push(node.value.as_ref());
        }
        Stmt::AnnAssign(node) => {
            out.push(node.target.as_ref());
            out.push(node.annotation.as_ref());
            out.extend(node.value.as_deref());
        }
        Stmt::For(node) => {
            out.push(node.target.as_ref());
            out.push(node.iter.as_ref());
        }
        Stmt::AsyncFor(node) => {
            out.push(node.target.as_ref());
            out.push(node.iter.as_ref());
        }
        Stmt::While(node) => out.push(node.test.as_ref()),
        Stmt::If(node) => out.push(node.test.as_ref()),
        Stmt::With(node) => push_with_items(&node.items, out),
        Stmt::AsyncWith(node) => push_with_items(&node.items, out),
        Stmt::Match(node) => {
            out.push(node.subject.as_ref());
            out.extend(node.cases.iter().filter_map(|case| case.guard.as_deref()));
        }
        Stmt::Raise(node) => {
            out.extend(node.exc.as_deref());
            out.extend(node.cause.as_deref());
        }
        Stmt::Try(node) => push_handler_types(&node.handlers, out),
        Stmt::TryStar(node) => push_handler_types(&node.handlers, out),
        Stmt::Assert(node) => {
            out.push(node.test.as_ref());
            out.extend(node.msg.as_deref());
        }
        Stmt::Expr(node) => out.push(node.value.as_ref()),
        Stmt::Import(_)
        | Stmt::ImportFrom(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_) => {}
    }
}

fn push_with_items<'a>(items: &'a [rustpython_parser::ast::WithItem], out: &mut Vec<&'a Expr>) {
    for item in items {
        out.push(&item.context_expr);
        out.extend(item.optional_vars.as_deref());
    }
}

fn push_handler_types<'a>(
    handlers: &'a [rustpython_parser::ast::ExceptHandler],
    out: &mut Vec<&'a Expr>,
) {
    for handler in handlers {
        let rustpython_parser::ast::ExceptHandler::ExceptHandler(handler) = handler;
        out.extend(handler.type_.as_deref());
    }
}

/// The statement bodies nested directly inside `stmt`.
///
/// The `bool` says whether the body opens a fresh binding scope — a function or
/// class body is not "inside" the loop that lexically contains it, and treating
/// it as such would report a defect at a point that runs once.
pub(crate) fn stmt_child_bodies(stmt: &Stmt) -> Vec<(&[Stmt], bool)> {
    match stmt {
        Stmt::FunctionDef(node) => vec![(node.body.as_slice(), true)],
        Stmt::AsyncFunctionDef(node) => vec![(node.body.as_slice(), true)],
        Stmt::ClassDef(node) => vec![(node.body.as_slice(), true)],
        Stmt::For(node) => vec![
            (node.body.as_slice(), false),
            (node.orelse.as_slice(), false),
        ],
        Stmt::AsyncFor(node) => vec![
            (node.body.as_slice(), false),
            (node.orelse.as_slice(), false),
        ],
        Stmt::While(node) => vec![
            (node.body.as_slice(), false),
            (node.orelse.as_slice(), false),
        ],
        Stmt::If(node) => vec![
            (node.body.as_slice(), false),
            (node.orelse.as_slice(), false),
        ],
        Stmt::With(node) => vec![(node.body.as_slice(), false)],
        Stmt::AsyncWith(node) => vec![(node.body.as_slice(), false)],
        Stmt::Match(node) => node
            .cases
            .iter()
            .map(|case| (case.body.as_slice(), false))
            .collect(),
        Stmt::Try(node) => try_bodies(&node.body, &node.handlers, &node.orelse, &node.finalbody),
        Stmt::TryStar(node) => {
            try_bodies(&node.body, &node.handlers, &node.orelse, &node.finalbody)
        }
        _ => Vec::new(),
    }
}

fn try_bodies<'a>(
    body: &'a [Stmt],
    handlers: &'a [rustpython_parser::ast::ExceptHandler],
    orelse: &'a [Stmt],
    finalbody: &'a [Stmt],
) -> Vec<(&'a [Stmt], bool)> {
    let mut bodies = vec![(body, false), (orelse, false), (finalbody, false)];
    for handler in handlers {
        let rustpython_parser::ast::ExceptHandler::ExceptHandler(handler) = handler;
        bodies.push((handler.body.as_slice(), false));
    }
    bodies
}

/// Every statement in the tree rooted at `body`, `body`'s own members included.
pub(crate) fn stmt_tree<'a>(body: &'a [Stmt]) -> Vec<&'a Stmt> {
    let mut found = Vec::new();
    let mut stack: Vec<&'a [Stmt]> = vec![body];
    while let Some(current) = stack.pop() {
        for stmt in current {
            found.push(stmt);
            for (child, _) in stmt_child_bodies(stmt) {
                stack.push(child);
            }
        }
    }
    found
}

/// The identifier of a plain `Name` expression, or `None`.
pub(crate) fn name_of(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(node) => Some(node.id.as_str()),
        _ => None,
    }
}

/// `true` if `expr` is the integer literal `0`.
pub(crate) fn is_zero_literal(expr: &Expr) -> bool {
    integer_literal(expr).is_some_and(|value| value == 0)
}

/// The value of a non-negative integer literal, or `None` for anything else.
pub(crate) fn integer_literal(expr: &Expr) -> Option<u128> {
    match expr {
        Expr::Constant(node) => match &node.value {
            rustpython_parser::ast::Constant::Int(value) => value.to_string().parse().ok(),
            _ => None,
        },
        _ => None,
    }
}

/// Names read by `expr`, minus anything a comprehension inside it binds.
///
/// Used to answer "does this expression depend on the loop variable?", which is
/// the difference between a loop-invariant rebuild and a per-item computation.
pub(crate) fn free_names(expr: &Expr) -> Vec<String> {
    let mut bound = Vec::new();
    let mut used = Vec::new();

    for node in expr_tree(expr) {
        match node {
            Expr::Name(name) if matches!(name.ctx, ExprContext::Load) => {
                used.push(name.id.as_str().to_owned());
            }
            Expr::ListComp(comp) => collect_generator_targets(&comp.generators, &mut bound),
            Expr::SetComp(comp) => collect_generator_targets(&comp.generators, &mut bound),
            Expr::DictComp(comp) => collect_generator_targets(&comp.generators, &mut bound),
            Expr::GeneratorExp(comp) => collect_generator_targets(&comp.generators, &mut bound),
            _ => {}
        }
    }

    used.retain(|name| !bound.contains(name));
    used.sort_unstable();
    used.dedup();
    used
}

fn collect_generator_targets(
    generators: &[rustpython_parser::ast::Comprehension],
    out: &mut Vec<String>,
) {
    for generator in generators {
        target_names(&generator.target, out);
    }
}

/// Every name bound by an assignment target, flattening tuple and list targets.
pub(crate) fn target_names(target: &Expr, out: &mut Vec<String>) {
    for node in expr_tree(target) {
        if let Expr::Name(name) = node {
            out.push(name.id.as_str().to_owned());
        }
    }
}
