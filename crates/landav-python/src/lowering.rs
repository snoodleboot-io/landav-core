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
    sync::OnceLock,
};

use landav_bound::{Origin, Symbol};
use landav_fdk::{Signature, SignaturePack};
use landav_its::{
    ArithOp, CompareOp, CondId, Construct, DeclaredEffect, ExprId, Extent, Locals, RangeSpec,
    SourceProgramBuilder, StmtId, VarName, Writes,
};
use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt, text_size::TextRange};

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
    // Which names this module binds for itself, computed once. A signature pack
    // is keyed by *name*, and a module that writes `def isinstance(...)` is not
    // calling the builtin - see `SignaturePack` for the whole caveat and for
    // what this does not cover.
    let module_bound = bindings_of(&module);
    // `None` means "this scope binds names this pass could not enumerate", and
    // it disqualifies every signature rather than being silently treated as an
    // empty set.
    let mut lowered = Vec::new();
    for statement in &module {
        for definition in Definition::all_of(statement) {
            lowered.push(lower_function(
                path,
                &index,
                definition,
                module_bound.as_ref(),
            ));
        }
    }
    Ok(lowered)
}

/// The parts of a top-level definition that translating it depends on.
///
/// # Why a borrowed view and not two code paths
///
/// `def` and `async def` are two node types in the parser -
/// [`ast::StmtFunctionDef`] and [`ast::StmtAsyncFunctionDef`] - carrying
/// field-for-field the same thing, and nothing below this point has any reason
/// to know which it was handed: parameters are annotated the same way, the body
/// holds the same statements, and `async` describes how the function is
/// *called* rather than what its body costs. Every async-flavoured construct
/// that does cost something different - `await`, `async for`, `async with` -
/// is already refused where it stands, by the statement and expression arms
/// that a plain `def` reaches too.
///
/// `LAN-93` was that the module walk matched only the first of the two, so an
/// `async def` produced no [`LoweredFunction`]: not refused, not holed, not in
/// the coverage denominator. Reaching both by duplicating [`lower_function`],
/// [`integer_names`], [`integer_parameters`] and [`collection_parameters`]
/// would fix the count and leave four pairs of functions to drift apart - and
/// the copy that drifted would be the async one, which is the one nobody reads.
/// One view, constructed once, keeps a single body of logic.
///
/// Borrowed and [`Copy`]: the AST stays the sole owner of every part, and
/// building one of these costs nothing.
///
/// # A method is a definition too - `LAN-104`
///
/// `lower_module` used to hand over only the definitions in `module.body`, and
/// a `ClassDef` is not one, so a method was never a [`LoweredFunction`]: not
/// refused, not holed, not in the coverage denominator - the silence `LAN-93`
/// closed for `async def`, one nesting level down. Measured on the typed
/// corpus, that was 35,413 methods against 7,790 module-level functions, and
/// 69% of the loops. [`Definition::all_of`] yields each `def` directly inside
/// a module-level class, carrying the class's name so a report says
/// `Class.method`.
///
/// A method body is a function body and the same translator runs over it.
/// Nothing is claimed about `self`: it is an unannotated parameter, reads of
/// `self.x` are `Attribute` refusals and stores are `complex-assignment-target`
/// refusals, exactly as for any other object parameter. A decorator changes
/// how the function is *called*, not what its body costs, so `@classmethod`,
/// `@staticmethod` and `@property` are lowered as any method is.
///
/// # What a method does not see
///
/// A name bound in the **class body** is not visible inside a method as a bare
/// name - Python resolves `isinstance(x, int)` in a method against the module
/// and the builtins, never against a class attribute of that name - so the
/// class body's bindings do **not** join the shadowing set. The module's do,
/// and the method's own do, as for any function.
///
/// # What stays out
///
/// A `def` nested inside a function, and a class nested inside a function or
/// another class. Both are reached only through a scope this walk does not
/// enter, and a test pins that the omission is a decision.
#[derive(Clone, Copy)]
struct Definition<'a> {
    /// The name as written.
    name: &'a str,
    /// The class this is a method of, if any. `None` for a module-level
    /// function.
    class: Option<&'a str>,
    /// The parameter list, which is where `int` and collection annotations are
    /// read from.
    args: &'a ast::Arguments,
    /// The statements to translate.
    body: &'a [Stmt],
    /// The extent of the **whole statement**. For an `async def` that starts at
    /// the `async` keyword, not at the `def` five characters later, so a report
    /// points at the line as the reader sees it.
    range: TextRange,
}

impl<'a> Definition<'a> {
    /// The definition `statement` is, or `None` if it is not one.
    ///
    /// Both arms live here, together, deliberately: this function is the only
    /// place that decides what counts as a top-level function, so a node type
    /// missing from it is missing from the denominator and says nothing about
    /// itself anywhere in the output. That silence is the whole of `LAN-93`.
    fn of(statement: &'a Stmt) -> Option<Self> {
        match statement {
            Stmt::FunctionDef(function) => Some(Self {
                name: function.name.as_str(),
                class: None,
                args: function.args.as_ref(),
                body: &function.body,
                range: function.range,
            }),
            Stmt::AsyncFunctionDef(function) => Some(Self {
                name: function.name.as_str(),
                class: None,
                args: function.args.as_ref(),
                body: &function.body,
                range: function.range,
            }),
            _ => None,
        }
    }

    /// Every definition a module-level `statement` holds: the function it is,
    /// or the methods of the class it is. See the type's doc for `LAN-104`.
    fn all_of(statement: &'a Stmt) -> Vec<Self> {
        if let Some(function) = Self::of(statement) {
            return vec![function];
        }
        let Stmt::ClassDef(class) = statement else {
            return Vec::new();
        };
        class
            .body
            .iter()
            .filter_map(Self::of)
            .map(|method| Self {
                class: Some(class.name.as_str()),
                ..method
            })
            .collect()
    }

    /// The name a report uses: `Class.method` for a method, the bare name
    /// otherwise.
    fn qualified_name(&self) -> String {
        match self.class {
            Some(class) => format!("{class}.{}", self.name),
            None => self.name.to_owned(),
        }
    }
}

impl Ranged for Definition<'_> {
    fn range(&self) -> TextRange {
        self.range
    }
}

/// Translates one `def` or `async def`.
fn lower_function(
    path: &Path,
    index: &LineIndex,
    function: Definition<'_>,
    module_bound: Option<&BTreeSet<String>>,
) -> LoweredFunction {
    let integers = integer_names(function);
    let name = function.qualified_name();
    let class = function.class.map(str::to_owned);
    let location = position(path, index, &function);
    let origin = origin_of(path, index, &function);

    let collections: BTreeSet<String> = collection_parameters(function).into_iter().collect();

    // A collection parameter contributes its **length** rather than itself: the
    // fragment has no value for a list, and the length is the only thing about
    // one that a cost can be a function of. `LAN-89`.
    let params: Vec<VarName> = integer_parameters(function)
        .into_iter()
        .map(VarName::new)
        .chain(
            collection_parameters(function)
                .iter()
                .map(|it| length_var(it)),
        )
        .collect();

    let mut builder = SourceProgramBuilder::new(name.clone(), origin, params);
    // A length is a property of an object, not a value bound to a name: a
    // `property` getter or a `__getitem__` is free to call `items.append(...)`,
    // so foreign code changes what `len(items)` denotes without rebinding
    // anything. An integer parameter cannot be moved that way. Core needs the
    // difference to decide what survives a read; see
    // `landav_its::SourceProgram::is_volatile`.
    for collection in collection_parameters(function) {
        builder.mark_volatile(length_var(&collection));
    }

    // Every name that is bound anywhere this call site can see it: the module's
    // own bindings, this function's parameters, and everything its body binds.
    // A callee found here gets no signature, whatever the pack says.
    // What the body binds, on its own: a parameter never named here still
    // holds what the caller passed, which is what lets a method row match its
    // receiver (`LAN-103`). `None` when the body could not be enumerated, and
    // that refuses every receiver, as it refuses every callee below.
    let rebound = bindings_of(function.body);
    let shadowed = module_bound
        .zip(rebound.clone())
        .map(|(module_names, mut names)| {
            names.extend(module_names.iter().cloned());
            names.extend(parameter_names(function));
            names
        });

    let mut translator = Translator {
        path,
        index,
        builder,
        integers,
        collections,
        shadowed,
        rebound,
        pack: builtin_pack(),
        walks: 0,
        discarding: false,
        value_discarded: false,
        truth_test: false,
    };
    let body = translator.block(function.body);
    let program = translator.builder.build(body);

    LoweredFunction::new(name, class, location, program)
}

// ---------------------------------------------------------------------------
// the signature pack, and the names that disqualify it
// ---------------------------------------------------------------------------

/// The OSS builtin signature pack, read once.
///
/// Parsed lazily rather than per module: `lower_module` runs once per file and
/// a corpus is thousands of files, so re-reading the same TOML each time would
/// be the analysis's own hot loop.
///
/// A pack that fails to parse yields an **empty** pack rather than a panic, and
/// an empty pack resolves nothing: every call stays the hole it was before this
/// ticket. Failing closed is the only failure mode available to a library that
/// may not abort, and it is the right one - the cost of a broken pack is lost
/// coverage, never an unsound bound. `landav-fdk`'s own tests keep the shipped
/// file parsing.
fn builtin_pack() -> &'static SignaturePack {
    static PACK: OnceLock<SignaturePack> = OnceLock::new();
    PACK.get_or_init(|| SignaturePack::builtin().unwrap_or_default())
}

/// The declaration a resolvable signature makes about a call site.
fn effect_of(row: &Signature) -> DeclaredEffect {
    DeclaredEffect::new(row.cost.steps(), row.rebinds_locals, row.mutates_arguments)
}

/// Every name a parameter list binds.
fn parameter_names(function: Definition<'_>) -> Vec<String> {
    let arguments = function.args;
    arguments
        .posonlyargs
        .iter()
        .chain(arguments.args.iter())
        .chain(arguments.kwonlyargs.iter())
        .map(|parameter| parameter.def.arg.to_string())
        .chain(
            arguments
                .vararg
                .iter()
                .chain(arguments.kwarg.iter())
                .map(|parameter| parameter.arg.to_string()),
        )
        .collect()
}

/// Every name `statements` bind **in their own scope**.
///
/// # What this is for
///
/// A signature pack is keyed by callee name, and matching by name is unsound
/// the moment a name means something else. `def isinstance(x, t): ...`,
/// `from mymod import isinstance` and `isinstance = my_check` all rebind it, and
/// a pack that resolved the call anyway would be publishing a bounded cost for
/// code it has never seen. This is the set that disqualifies a row - see
/// [`landav_fdk::SignaturePack`], where the whole caveat is written including
/// the one case this cannot catch.
///
/// # Why it does not descend into a nested definition
///
/// A nested `def` or `class` binds its own *name* in this scope, and that name
/// is recorded. What its body binds is local to it and cannot change what a name
/// means out here, so descending would over-refuse for no soundness gain. Every
/// other block - `if`, `for`, `while`, `with`, `try` - shares this scope and is
/// descended into.
///
/// # Deliberately over-approximate
///
/// A name bound on one branch of an `if` counts as bound. Being wrong in this
/// direction costs a signature and nothing else.
fn bindings_of(statements: &[Stmt]) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut work: Vec<&Stmt> = statements.iter().rev().collect();
    while let Some(statement) = work.pop() {
        match statement {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    collect_target_names(target, &mut names);
                }
            }
            Stmt::AnnAssign(assign) => collect_target_names(&assign.target, &mut names),
            Stmt::AugAssign(assign) => collect_target_names(&assign.target, &mut names),
            Stmt::FunctionDef(node) => {
                names.insert(node.name.to_string());
            }
            Stmt::AsyncFunctionDef(node) => {
                names.insert(node.name.to_string());
            }
            Stmt::ClassDef(node) => {
                names.insert(node.name.to_string());
            }
            Stmt::Import(node) => {
                for alias in &node.names {
                    // `import a.b` binds `a`; `import a.b as c` binds `c`.
                    let bound = alias.asname.as_ref().map_or_else(
                        || {
                            alias
                                .name
                                .split('.')
                                .next()
                                .unwrap_or(alias.name.as_str())
                                .to_owned()
                        },
                        |name| name.to_string(),
                    );
                    names.insert(bound);
                }
            }
            Stmt::ImportFrom(node) => {
                for alias in &node.names {
                    // `from m import *` binds names this pass cannot enumerate.
                    // Recorded as the residual risk in `SignaturePack` rather
                    // than guessed at here.
                    let bound = alias
                        .asname
                        .as_ref()
                        .map_or_else(|| alias.name.to_string(), |name| name.to_string());
                    names.insert(bound);
                }
            }
            Stmt::Global(node) => names.extend(node.names.iter().map(ToString::to_string)),
            Stmt::Nonlocal(node) => names.extend(node.names.iter().map(ToString::to_string)),
            Stmt::For(node) => {
                collect_target_names(&node.target, &mut names);
                work.extend(node.body.iter().chain(node.orelse.iter()));
            }
            Stmt::AsyncFor(node) => {
                collect_target_names(&node.target, &mut names);
                work.extend(node.body.iter().chain(node.orelse.iter()));
            }
            Stmt::While(node) => work.extend(node.body.iter().chain(node.orelse.iter())),
            Stmt::If(node) => work.extend(node.body.iter().chain(node.orelse.iter())),
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        collect_target_names(target, &mut names);
                    }
                }
                work.extend(node.body.iter());
            }
            Stmt::AsyncWith(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        collect_target_names(target, &mut names);
                    }
                }
                work.extend(node.body.iter());
            }
            Stmt::Try(node) => {
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(clause) = handler;
                    if let Some(name) = &clause.name {
                        names.insert(name.to_string());
                    }
                    work.extend(clause.body.iter());
                }
                work.extend(
                    node.body
                        .iter()
                        .chain(node.orelse.iter())
                        .chain(node.finalbody.iter()),
                );
            }
            Stmt::TryStar(node) => {
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(clause) = handler;
                    if let Some(name) = &clause.name {
                        names.insert(name.to_string());
                    }
                    work.extend(clause.body.iter());
                }
                work.extend(
                    node.body
                        .iter()
                        .chain(node.orelse.iter())
                        .chain(node.finalbody.iter()),
                );
            }
            // A `case` pattern binds capture names, and enumerating them needs
            // the pattern grammar this pass does not walk. Rather than guess,
            // the whole scope answers "names I could not enumerate", which
            // disqualifies every signature in it. `match` is refused as a
            // construct anyway, so the coverage this costs is a function that
            // was already blocked.
            Stmt::Match(_) => return None,
            Stmt::TypeAlias(node) => collect_target_names(&node.name, &mut names),
            Stmt::Return(_)
            | Stmt::Delete(_)
            | Stmt::Raise(_)
            | Stmt::Assert(_)
            | Stmt::Expr(_)
            | Stmt::Pass(_)
            | Stmt::Break(_)
            | Stmt::Continue(_) => {}
        }
    }
    Some(names)
}

/// Every plain name an assignment target binds, through tuples and stars.
fn collect_target_names(target: &Expr, names: &mut BTreeSet<String>) {
    let mut work = vec![target];
    while let Some(node) = work.pop() {
        match node {
            Expr::Name(name) => {
                names.insert(name.id.to_string());
            }
            Expr::Tuple(tuple) => work.extend(tuple.elts.iter()),
            Expr::List(list) => work.extend(list.elts.iter()),
            Expr::Starred(starred) => work.push(starred.value.as_ref()),
            // `obj.field = ...` and `table[k] = ...` bind no name.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// which names are integers
// ---------------------------------------------------------------------------

/// The parameters annotated `int`, in declaration order.
fn integer_parameters(function: Definition<'_>) -> Vec<String> {
    let arguments = function.args;
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

/// The builtin types whose `len` counts exactly the values `for` walks.
///
/// The equality is the point. `len` and iteration have to agree, or a loop
/// counted by the length runs a different number of times than the bound says.
/// For each of these Python guarantees they do: a `dict` iterates its keys and
/// there are `len(d)` of them, a `str` iterates its characters and there are
/// `len(s)` of them.
///
/// Deliberately absent: `Iterable`, `Iterator`, `Generator` and `Sequence`. The
/// first three need not have a `len` at all and iterating one may consume it;
/// `Sequence` is a protocol any user class may claim, and its `__len__` and
/// `__iter__` are two methods that need not agree.
const SIZED_BUILTINS: [&str; 7] = ["list", "tuple", "set", "frozenset", "dict", "str", "bytes"];

/// Whether an annotation names a builtin collection whose length bounds
/// iteration over it.
///
/// Accepts the bare name and the subscripted form, so `list` and `list[int]`
/// are both collections - the element type says nothing about how many there
/// are, which is the only question here.
///
/// This trusts the annotation exactly as far as [`annotation_is_int`] trusts
/// `int`: Python does not enforce either at runtime, and a frontend that
/// declines to trust annotations has nothing left to reason from.
fn annotation_is_collection(annotation: Option<&Expr>) -> bool {
    let named = match annotation {
        Some(Expr::Name(name)) => name.id.as_str(),
        // `list[int]`, `dict[str, int]` - the value is what is subscripted.
        Some(Expr::Subscript(subscript)) => match subscript.value.as_ref() {
            Expr::Name(name) => name.id.as_str(),
            _ => return false,
        },
        _ => return false,
    };
    SIZED_BUILTINS.contains(&named)
}

/// The parameters annotated with a sized builtin collection, in declaration
/// order.
///
/// # A parameter a loop or a `with` rebinds is not one
///
/// `items: list` rebound by `for items in rows:` holds, inside and after that
/// loop, whatever `rows` yielded - and the length the caller supplied is not a
/// fact about it any more. The program records no such binding: a collection
/// walk counts on a synthetic counter and binds its target to nothing (see
/// [`Translator::walk_collection`]), and a `with ... as items` is the same.
/// So `len(items)` stayed readable across them, and a loop over `items` below
/// counted by the caller's length. `LAN-101`: such a parameter has no length
/// variable at all, so nothing downstream can read one.
///
/// # A parameter an *assignment* rebinds still is one
///
/// Deliberately, and measured. `items = []` is a refused binding carrying
/// `LAN-100`'s write set - `items` and `len(items)` both, see
/// [`Translator::rebound_names`] - so the length is forgotten exactly where
/// the program changes it, and a loop over `items` *above* the assignment is
/// still counted. Excluding assignments here as well was tried and cost three
/// of the typed corpus's fifty counted loops, in functions that rebind the
/// parameter after or inside the loop, for no soundness gain. The rule is:
/// a rebinding the program records at a statement is handled there; one it
/// does not record is handled by never declaring the length.
///
/// The walk does not descend into a nested `def` - a binding there is that
/// scope's, not this one's - and a body it cannot enumerate (`match`)
/// disqualifies every parameter, which costs coverage and never soundness.
fn collection_parameters(function: Definition<'_>) -> Vec<String> {
    let arguments = function.args;
    let rebound = target_bindings_of(function.body);
    arguments
        .posonlyargs
        .iter()
        .chain(arguments.args.iter())
        .filter(|parameter| annotation_is_collection(parameter.def.annotation.as_deref()))
        .map(|parameter| parameter.def.arg.to_string())
        .filter(|name| !rebound.as_ref().is_none_or(|names| names.contains(name)))
        .collect()
}

/// The bound variable standing for the length of collection parameter `name`.
///
/// Spelled `len(items)` because that is what the caller would write to compute
/// it, and because no Python identifier contains a parenthesis - so this can
/// never collide with a name the source could have bound. The same reasoning
/// `Hole` uses for `#hole0`.
fn length_var(name: &str) -> VarName {
    VarName::new(format!("len({name})"))
}

/// The names bound by a `for` target or a `with ... as` target anywhere in
/// `statements`, or `None` if a statement binds names this walk cannot read.
///
/// The subset of [`bindings_of`] that [`collection_parameters`] needs: the
/// binding forms the translated program does **not** record at a statement.
/// Same descent - every block that shares this scope, never a nested
/// definition - and the same answer for a `match`.
fn target_bindings_of(statements: &[Stmt]) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut work: Vec<&Stmt> = statements.iter().rev().collect();
    while let Some(statement) = work.pop() {
        match statement {
            Stmt::For(node) => {
                collect_target_names(&node.target, &mut names);
                work.extend(node.body.iter().chain(node.orelse.iter()));
            }
            Stmt::AsyncFor(node) => {
                collect_target_names(&node.target, &mut names);
                work.extend(node.body.iter().chain(node.orelse.iter()));
            }
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        collect_target_names(target, &mut names);
                    }
                }
                work.extend(node.body.iter());
            }
            Stmt::AsyncWith(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        collect_target_names(target, &mut names);
                    }
                }
                work.extend(node.body.iter());
            }
            Stmt::While(node) => work.extend(node.body.iter().chain(node.orelse.iter())),
            Stmt::If(node) => work.extend(node.body.iter().chain(node.orelse.iter())),
            Stmt::Try(node) => {
                work.extend(node.body.iter());
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(handler) = handler;
                    work.extend(handler.body.iter());
                }
                work.extend(node.orelse.iter().chain(node.finalbody.iter()));
            }
            Stmt::TryStar(node) => {
                work.extend(node.body.iter());
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(handler) = handler;
                    work.extend(handler.body.iter());
                }
                work.extend(node.orelse.iter().chain(node.finalbody.iter()));
            }
            Stmt::Match(_) => return None,
            // Every assignment form is a statement the program records with
            // its own write set; a nested definition is its own scope.
            _ => {}
        }
    }
    Some(names)
}

/// The names that provably hold integers throughout `function`.
///
/// A least fixed point: start with every assigned name plus the `int`
/// parameters, then repeatedly drop any name with an assignment whose
/// right-hand side is not an integer under the current set. Shrinking only, so
/// it terminates in at most as many rounds as there are names.
fn integer_names(function: Definition<'_>) -> BTreeSet<String> {
    let arguments = function.args;
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

    let collections: BTreeSet<String> = collection_parameters(function).into_iter().collect();
    let mut candidates: BTreeSet<String> = integer_parameters(function).into_iter().collect();
    for statement in crate::syntax::stmt_tree(function.body) {
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
        for statement in crate::syntax::stmt_tree(function.body) {
            collect_non_integer_bindings(statement, &candidates, &collections, &mut doomed);
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
                written_names(target, &mut names);
            }
        }
        Stmt::AugAssign(assign) => written_names(&assign.target, &mut names),
        Stmt::AnnAssign(assign) => written_names(&assign.target, &mut names),
        Stmt::For(loop_stmt) => written_names(&loop_stmt.target, &mut names),
        _ => {}
    }
    names
}

/// The names an assignment to `target` changes the meaning of.
///
/// # Why this is not every name the target mentions
///
/// `a[i] = 0` writes through `a` and **reads** `i`. Walking the whole target
/// expression cannot tell those apart, so it condemned the index alongside the
/// container - and when the index is a loop counter, condemning it makes the
/// counter a non-integer, which refuses the whole `for` statement, throws away
/// its body, and takes any loop nested inside that body out of the program with
/// it. Measured: 29 of the standard library's 166 `range` loops write through
/// their own counter, and every one of them was refused whole. The same write
/// with a literal index kept its count, which is the shape of the bug.
///
/// Condemning the container is right - the analysis cannot see what `a` holds
/// afterwards. Condemning the index is not: a read of a name proves nothing
/// about it either way.
fn written_names(target: &Expr, out: &mut Vec<String>) {
    match target {
        Expr::Name(name) => out.push(name.id.as_str().to_owned()),
        // The object whose state changes, never the index or the attribute.
        Expr::Subscript(subscript) => written_names(&subscript.value, out),
        Expr::Attribute(attribute) => written_names(&attribute.value, out),
        Expr::Tuple(tuple) => {
            for element in &tuple.elts {
                written_names(element, out);
            }
        }
        Expr::List(list) => {
            for element in &list.elts {
                written_names(element, out);
            }
        }
        Expr::Starred(starred) => written_names(&starred.value, out),
        // `f()[0] = 1` changes nothing this analysis names.
        _ => {}
    }
}

/// Adds any name this statement binds to a non-integer value.
fn collect_non_integer_bindings(
    statement: &Stmt,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
    doomed: &mut BTreeSet<String>,
) {
    let mut condemn = |target: &Expr| {
        let mut names = Vec::new();
        written_names(target, &mut names);
        for name in names {
            if candidates.contains(&name) {
                doomed.insert(name);
            }
        }
    };

    match statement {
        Stmt::Assign(assign) => {
            if !is_integer_expr(&assign.value, candidates, collections) {
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
            if !arithmetic || !is_integer_expr(&assign.value, candidates, collections) {
                condemn(&assign.target);
            }
        }
        Stmt::AnnAssign(assign) => {
            if !annotation_is_int(Some(&assign.annotation)) {
                condemn(&assign.target);
            }
            if let Some(value) = &assign.value
                && !is_integer_expr(value, candidates, collections)
            {
                condemn(&assign.target);
            }
        }
        Stmt::For(loop_stmt)
            if range_arguments(&loop_stmt.iter).is_none_or(|args| {
                !args
                    .iter()
                    .all(|arg| is_integer_expr(arg, candidates, collections))
            }) =>
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
fn is_integer_expr(
    expr: &Expr,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
) -> bool {
    match expr {
        // `len(items)` is a natural number, so a name bound from it is an
        // integer and a `range` over it has an integer counter. `LAN-89`.
        Expr::Call(call) => length_of_collection(call, collections).is_some(),
        Expr::Constant(constant) => {
            matches!(constant.value, Constant::Int(_) | Constant::Bool(_))
        }
        Expr::Name(name) => candidates.contains(name.id.as_str()),
        Expr::BinOp(binary) => match binary.op {
            ast::Operator::Add | ast::Operator::Sub | ast::Operator::Mult => {
                is_integer_expr(&binary.left, candidates, collections)
                    && is_integer_expr(&binary.right, candidates, collections)
            }
            ast::Operator::Pow => {
                literal_exponent(&binary.right).is_some()
                    && is_integer_expr(&binary.left, candidates, collections)
            }
            // `//`, `%`, `<<` and `>>` over integers yield integers. This has
            // to agree with `build_expression`, and it is the *same* predicate
            // that decides both: proving `x = n // 2` binds an integer is what
            // stops `x` being condemned, and a condemned `x` refuses at the
            // binding, at every later read of it, and at the `for` line of any
            // range it appears in. Teaching the translation alone would leave
            // those three refusals standing. `LAN-91`.
            ast::Operator::FloorDiv
            | ast::Operator::Mod
            | ast::Operator::LShift
            | ast::Operator::RShift => approximation_of(binary, candidates, collections).is_some(),
            _ => false,
        },
        Expr::UnaryOp(unary) => {
            matches!(unary.op, ast::UnaryOp::USub | ast::UnaryOp::UAdd)
                && is_integer_expr(&unary.operand, candidates, collections)
        }
        // `a if c else b` is an integer when both arms are, whichever way the
        // test goes. Without this arm `x = a if c else b` never proves `x` an
        // integer, so the binding refuses as `non-integer-value` **on top of**
        // the conditional-expression refusal, and so does every later read of
        // `x` and every `range` mentioning it. Teaching the translation without
        // teaching this leaves two of the three refusals standing. `LAN-91`.
        //
        // It does not make the ternary's *value* available: nothing here claims
        // to know which arm ran, and `Translator::bind_expression` binds the
        // name inside a branch rather than to an expression.
        Expr::IfExp(ternary) => {
            is_integer_expr(&ternary.body, candidates, collections)
                && is_integer_expr(&ternary.orelse, candidates, collections)
        }
        _ => false,
    }
}

/// The arguments of a `range(...)` call, or `None` if this is not one.
/// The collection parameter whose length this call computes, if it is one.
///
/// `len(items)` and nothing else: a keyword argument, a second argument, or an
/// argument that is not a bare name all mean this is some other call. `len` is
/// matched by name because the fragment has no way to know it was rebound, and
/// a module that shadows `len` is doing something this analysis cannot follow
/// anyway - the same trust the `int` annotation already gets.
fn length_of_collection(call: &ast::ExprCall, collections: &BTreeSet<String>) -> Option<String> {
    let Expr::Name(callee) = call.func.as_ref() else {
        return None;
    };
    if callee.id.as_str() != "len" || !call.keywords.is_empty() {
        return None;
    }
    let [Expr::Name(argument)] = call.args.as_slice() else {
        return None;
    };
    collections
        .contains(argument.id.as_str())
        .then(|| argument.id.to_string())
}

/// How many values a display iterates, and whether that count is exact.
///
/// # Only a display, and only some of them
///
/// | form | length | why |
/// |---|---|---|
/// | `[a, b, c]`, `(a, b, c)`, `'abc'`, `b'abc'` | exactly 3 | positional, nothing collapses |
/// | `{a, b, c}`, `{a: 1, b: 2}` | **at most** 3 | equal elements and equal keys collapse |
/// | `[*rest, 1]` | not known | `rest` contributes `len(rest)`, and this cannot see it |
/// | `[0] * n`, `a + b`, a comprehension | not known | not a display at all |
///
/// The set and dict rows are the reason this returns exactness rather than a
/// number. `{1, 1, 2}` writes three elements and iterates two, so counting three
/// is a sound upper bound and a **false** `Theta` - and `Theta` against `O` is a
/// distinction this codebase reports to its users. `[*rest, 1]` is the sharper
/// trap: it is an ordinary `Expr::List` with two elements, so "the trip count is
/// `elts.len()`" answers 2 for a loop that runs `len(rest) + 1` times.
///
/// A `str` is measured in **characters**, because that is what Python iterates.
fn walked_display(expr: &Expr) -> Option<(u64, bool)> {
    let counted = |elements: &[Expr], exact: bool| -> Option<(u64, bool)> {
        if elements.iter().any(|it| matches!(it, Expr::Starred(_))) {
            return None;
        }
        Some((u64::try_from(elements.len()).ok()?, exact))
    };
    match expr {
        Expr::List(list) => counted(&list.elts, true),
        Expr::Tuple(tuple) => counted(&tuple.elts, true),
        Expr::Set(set) => counted(&set.elts, false),
        // A `None` key is `**mapping`, which spreads an unknown number of them.
        Expr::Dict(dict) => {
            if dict.keys.iter().any(Option::is_none) {
                return None;
            }
            Some((u64::try_from(dict.keys.len()).ok()?, false))
        }
        Expr::Constant(constant) => match &constant.value {
            Constant::Str(text) => Some((u64::try_from(text.chars().count()).ok()?, true)),
            Constant::Bytes(bytes) => Some((u64::try_from(bytes.len()).ok()?, true)),
            Constant::Tuple(elements) => Some((u64::try_from(elements.len()).ok()?, true)),
            _ => None,
        },
        _ => None,
    }
}

/// How many values an iterable yields, where a signature row says so.
///
/// The count is *always* the length of a collection parameter - that is the only
/// length this fragment has a bound variable for - and the relation is what
/// connects the iterable to it. `sorted(records)` and `enumerate(sorted(records))`
/// both answer `records`; `set(records)` answers `records` inexactly.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LengthWalk {
    /// The collection parameter whose length bounds the walk.
    collection: String,
    /// Whether that length is the count itself rather than an upper bound on
    /// it. Conjunctive down the chain: one collapsing callee makes the whole
    /// reading inexact, and an inexact reading is reported as `O` and never as
    /// `Theta`.
    exact: bool,
}

/// The collection parameter this expression iterates, if it is a bare name for
/// one.
fn walked_collection(expr: &Expr, collections: &BTreeSet<String>) -> Option<String> {
    let Expr::Name(name) = expr else {
        return None;
    };
    collections
        .contains(name.id.as_str())
        .then(|| name.id.to_string())
}

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

/// How deeply nested a conditional expression may be before it is refused.
///
/// Splitting `a if c else b` into a branch is one recursive frame per level,
/// and `a if c else b if d else e` chains to the right without a bracket - so
/// the byte-level guard, which counts brackets and operator characters, does
/// not bound this. Sixty-four is far past anything a human writes and far short
/// of a stack that runs out; past it the ternary is refused, which is the same
/// answer the frontend gave before this ticket and is sound for the same reason.
const MAX_TERNARY_DEPTH: u32 = 64;

/// The largest left-shift this frontend turns into a multiplication.
///
/// `a << k` is `a * 2^k` exactly, and the fragment's literals are `i64`, so the
/// factor has to fit one: `2^62` does and `2^63` does not. Past it the operator
/// is refused rather than saturated - a wrapped factor is a *smaller* number
/// than the truth, which is the one direction a bound may never move.
const MAX_SHIFT: u32 = 62;

/// What can be said about an integer operator the polynomial fragment cannot
/// express.
///
/// Only these two shapes, and the difference between them is the trap in this
/// whole lane. `<<` is the only one of the seven whose result is **larger** than
/// its operand, so handling `Construct::BitwiseOperator` uniformly - bounding
/// `n << 1` by `n` because `n >> 1` is bounded by `n` - under-reports by a
/// factor of two per shift.
#[derive(Debug, Clone, Copy)]
enum Approximated {
    /// `a << k` is exactly `a * 2^k`; the factor is that power of two.
    ///
    /// Exact, so it stays a plain product and the loops counted by it stay
    /// `Theta`.
    Scaled(i64),
    /// The value's magnitude is dominated by the **left** operand's.
    ///
    /// `|a // b| <= |a|`, `|a % b| <= |a|` and `|a >> k| <= |a|` whenever `a` is
    /// non-negative, for every `b != 0` and every `k >= 0`; a divisor of zero or
    /// a negative shift raises before the value is used, and a negative result
    /// makes a `range` over it empty, so an upper bound over `a` holds in every
    /// case.
    ///
    /// # The divisor may not appear
    ///
    /// Division is **anti-monotone** in its second operand while every
    /// `landav_bound::Bound` is weakly monotone, so a bound mentioning the
    /// divisor takes its smallest value exactly where the true quotient takes
    /// its largest: `100 // 1 == 100`, and a bound of `m` reads `1` there. The
    /// inequality above mentions `b` nowhere, which is precisely what makes it
    /// safe to write down.
    ByDividend,
}

/// How `binary` may be approximated, or `None` if it may not be.
///
/// # Both operands must be proven integers first
///
/// Not a formality. It is what lets the *right* operand be left untranslated:
/// an expression this predicate accepts is built from literals, proven-integer
/// names and `+ - * **`, so it contains no call, no attribute and no subscript,
/// and therefore no cost and no effect that dropping it could hide. The left
/// operand is a different matter - it is kept, referenced by the refusal node,
/// and walked - so a region inside it is charged where it stands.
fn approximation_of(
    binary: &ast::ExprBinOp,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
) -> Option<Approximated> {
    if !is_integer_expr(&binary.left, candidates, collections)
        || !is_integer_expr(&binary.right, candidates, collections)
    {
        return None;
    }
    match binary.op {
        ast::Operator::FloorDiv | ast::Operator::Mod | ast::Operator::RShift => {
            Some(Approximated::ByDividend)
        }
        ast::Operator::LShift => {
            let shift = literal_shift(&binary.right)?;
            // `1 << 62` is the largest power of two an `i64` holds.
            (shift <= MAX_SHIFT).then(|| Approximated::Scaled(1_i64 << shift))
        }
        _ => None,
    }
}

/// A non-negative literal shift amount.
///
/// A negative one raises `ValueError` in Python and would panic a shift here,
/// so it is not a literal this recognises; a symbolic one makes the result
/// `n * 2^m`, which is not a polynomial and has no `SourceExpr`.
fn literal_shift(expr: &Expr) -> Option<u32> {
    let Expr::Constant(constant) = expr else {
        return None;
    };
    let Constant::Int(value) = &constant.value else {
        return None;
    };
    u32::try_from(value.clone()).ok()
}

/// How the frontend spells an operator it approximates, for the report.
const fn spelling_of(op: &ast::Operator) -> &'static str {
    match op {
        ast::Operator::FloorDiv => "//",
        ast::Operator::Mod => "%",
        ast::Operator::LShift => "<<",
        ast::Operator::RShift => ">>",
        ast::Operator::Div => "/",
        ast::Operator::Pow => "**",
        ast::Operator::BitAnd => "&",
        ast::Operator::BitOr => "|",
        ast::Operator::BitXor => "^",
        ast::Operator::MatMult => "@",
        ast::Operator::Add => "+",
        ast::Operator::Sub => "-",
        ast::Operator::Mult => "*",
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
    /// The parameters annotated with a sized builtin collection.
    ///
    /// Parameters only. A local holding a collection has no length the caller
    /// can supply, and one assigned from a region may have any length at all.
    collections: BTreeSet<String>,
    /// Every name the module and this function bind, or `None` when this pass
    /// could not enumerate them.
    ///
    /// A callee found here is not the builtin the signature pack is describing.
    /// `None` disqualifies every signature: see [`bindings_of`], and
    /// [`landav_fdk::SignaturePack`] for the residual case neither covers.
    shadowed: Option<BTreeSet<String>>,
    /// Every name this function's body binds, parameters excluded. A
    /// parameter absent from here holds what the caller passed at every point
    /// in the body, which is what a method row's receiver rule needs.
    /// `None` refuses every receiver: see [`bindings_of`].
    rebound: Option<BTreeSet<String>>,
    /// The signatures a call may be resolved against.
    pack: &'static SignaturePack,
    /// How many collection walks have been lowered, to keep their synthetic
    /// counters apart.
    walks: u32,
    /// Whether the expression being translated is evaluated for its **effect**
    /// and its value thrown away.
    ///
    /// True exactly while [`Translator::hoisted`] is running, and `hoisted`
    /// translates into a scratch builder that is discarded whole - only the
    /// refusals it found survive, and they survive through
    /// [`landav_its::SourceProgram::unsupported_nodes`], which *scans* the arena
    /// rather than walking it. Two things follow, and the collection lane rests
    /// on both:
    ///
    /// * a node built here never reaches the program, so a form whose value the
    ///   fragment cannot hold may return any placeholder at all rather than a
    ///   refusal - `x = [1, 2, 3]` refuses because `x` holds a list, not because
    ///   building the list was unanalysable, and an expression costs no source
    ///   step of its own;
    /// * an orphan is harmless here, because the scan finds it - so the
    ///   *elements* of such a form can be translated for their own refusals
    ///   without the parent having to reference them.
    ///
    /// Neither holds in a value position, which is why this is a flag and not a
    /// blanket change: `if [1, 2]:` is a truth test whose answer decides a
    /// branch, and `landav_its::lower` builds a guard out of it.
    discarding: bool,
    /// Whether the expression's value is read by **nothing at all**.
    ///
    /// Strictly narrower than [`Translator::discarding`], and the two are not
    /// the same question. `discarding` says the nodes go to a scratch builder,
    /// which is true of the right-hand side of `x = a and b` as well - that
    /// value is hoisted for its refusals only. But a binding *names* a value,
    /// and the report has to say why that name became unknowable: `x = a and b`
    /// yields `non-integer-value: x`, and without the `conditional-expression`
    /// beside it the user is told that `x` is not an integer and never told
    /// which construct made it one. So value position keeps its refusal, and
    /// `boolean_and_comparison_values_stay_refused` pins that.
    ///
    /// This flag is for the positions where there is no name and no value slot
    /// at all: a `return`, whose [`landav_its::SourceStmt::Return`] never
    /// represents what is returned, and a bare expression statement. There the
    /// only question a boolean, a comparison or a ternary raises is what it
    /// **costs**, and the answer is its operands' regions and nothing else.
    /// `return a and b` can no more publish a bound variable than `return [a]`
    /// can, because there is nowhere for the value to go.
    value_discarded: bool,
    /// Whether the expression being translated is the subject of a **truth
    /// test** rather than a value the fragment will read as a number.
    ///
    /// The third member of the family above, and the narrowest. `if f(x):`
    /// spells as `f(x) != 0` here because truthiness is a Python fact this
    /// crate owns, and the comparison is what the fragment holds - but nothing
    /// reads the *number*: the guard it produces is `either branch`, which is
    /// what `landav_its::lower` already emits for a condition it cannot
    /// translate.
    ///
    /// That is exactly the position where a signature may be resolved outside a
    /// scratch builder. `x = isinstance(a, b)` may not: the binding names a
    /// value, and this fragment has none for a `bool`. See
    /// [`Translator::declaration_for`].
    truth_test: bool,
}

impl Translator<'_> {
    fn origin<T: Ranged>(&self, node: &T) -> Origin {
        origin_of(self.path, self.index, node)
    }

    /// The position just past a node. See [`origin_past`].
    fn origin_after<T: Ranged>(&self, node: &T) -> Origin {
        origin_past(self.path, self.index, node)
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
                // *contain* something that has to be refused. It is translated
                // for that reason alone, and the refusals are hoisted to
                // statements **here**, at the position the expression is
                // evaluated - see `refusals_of`. Dropping the handle instead
                // left the refusal in the arena with nothing pointing at it,
                // which `landav_its::lower` still reported but which a consumer
                // walking the control structure could not place.
                let mut statements = match &ret.value {
                    Some(value) => self.discarded_cost_of(value),
                    None => Vec::new(),
                };
                let origin = self.origin(ret);
                statements.push(self.builder.return_stmt(origin));
                statements
            }
            Stmt::Pass(_) => Vec::new(),
            Stmt::Expr(bare) => self.bare_expression(bare),

            Stmt::Break(node) => vec![self.refuse_stmt(Construct::LoopJump, node)],
            Stmt::Continue(node) => vec![self.refuse_stmt(Construct::LoopJump, node)],
            Stmt::Raise(node) => self.raise(node),
            Stmt::Try(node) => self.try_statement(node),
            // `except*` runs **several** handlers for one exception group, so
            // the sum over its clauses is still the honest upper bound and the
            // shape is the same. It stays refused all the same: it is rare
            // enough that the corpus does not pay for it, and every construct
            // accepted here is soundness surface.
            Stmt::TryStar(node) => {
                vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)]
            }
            Stmt::Assert(node) => vec![self.refuse_stmt(Construct::ExceptionalControlFlow, node)],
            Stmt::With(node) => self.with_statement(node),
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

    /// `raise`, and the expression beside it.
    ///
    /// # One charged step, and an edge out of the region it stands in
    ///
    /// A `raise` used to be one whole-statement refusal, which was sound - the
    /// hole denotes `omega` - and cost the code around it everything: a region
    /// forgets every value the analysis knew, so a `raise` at the end of a
    /// `try` body erased the trip count of a loop in the handler beside it.
    ///
    /// It assigns to nothing and costs one step, so it is now spelled as
    /// [`landav_its::SourceStmt::Raise`]. **The step alone would be unsound**:
    /// `for i in range(n): raise ValueError` executes two steps for every `n`,
    /// and an engine charging the step without also learning that the loop can
    /// be left early reports `Theta(2n)` - complete, exact, no holes. The edge
    /// is the other half of this change and lives in `landav-engine`'s
    /// `exits_within`.
    ///
    /// # The expression is still translated
    ///
    /// `raise ValueError(compute(n))` runs a call, and a call has an unknown
    /// cost. Accepting the statement without translating what is inside it
    /// would leave that call in no arena at all - invisible to the refusal scan
    /// as well as to the walk - and the function would publish a bound that
    /// omits it. A bare class name is the exception: a global lookup runs no
    /// user code and binds nothing, so there is nothing to charge and nothing
    /// to refuse.
    fn raise(&mut self, node: &ast::StmtRaise) -> Vec<StmtId> {
        let mut statements = Vec::new();
        for expression in [node.exc.as_deref(), node.cause.as_deref()]
            .into_iter()
            .flatten()
        {
            if !is_name_lookup(expression) {
                statements.extend(self.refusals_of(expression));
            }
        }
        let origin = self.origin(node);
        statements.push(self.builder.raise_stmt(origin));
        statements
    }

    /// `try` / `except` / `else` / `finally`.
    ///
    /// Becomes one [`landav_its::SourceStmt::Protected`], whose cost rule -
    /// `body + handler + cleanup`, never exact - is stated there. What is
    /// decided *here* is which Python clause goes into which slot:
    ///
    /// * the `else` clause joins the **body**. It runs after the body exactly
    ///   when the body completed, so it is on the body's path and on no other;
    /// * every `except` clause joins the **handler**, concatenated. Exactly one
    ///   of them runs, so their sum dominates whichever it is;
    /// * `finally` is the **cleanup**, and the whole point of the slot is that
    ///   it sits outside the choice: charging it to the exceptional path only
    ///   understates every normal run by the entire `finally` body.
    ///
    /// # `except E as e` is a rebinding, and it is checked
    ///
    /// The engine forgets every readable name on the way *out* of a
    /// `Protected`, which covers a later trip count. It cannot cover one inside
    /// the handler itself: `except ValueError as n:` followed by
    /// `for i in range(n):` would read `n` as the caller's integer when it now
    /// holds an exception object. There is no expression for what it holds, so
    /// the clause opens with a refusal that forgets it - and only when the name
    /// is one this pass tracks, so the overwhelmingly common
    /// `except ValueError as e:` costs nothing.
    ///
    /// A collection parameter counts as tracked for the same reason: the bound
    /// its length contributes is a fact about the object that name denotes, and
    /// rebinding the name is not a change the length variable survives.
    fn try_statement(&mut self, node: &ast::StmtTry) -> Vec<StmtId> {
        let mut body = self.block(&node.body);
        body.extend(self.block(&node.orelse));

        let mut handler = Vec::new();
        for clause in &node.handlers {
            let ast::ExceptHandler::ExceptHandler(clause) = clause;
            // `except E:` evaluates `E` to decide whether the clause matches,
            // and `except self.errors:` runs a `property` getter to do it.
            if let Some(kind) = &clause.type_
                && !is_name_lookup(kind)
            {
                handler.extend(self.refusals_of(kind));
            }
            if let Some(name) = &clause.name
                && (self.integers.contains(name.as_str())
                    || self.collections.contains(name.as_str()))
            {
                handler.push(self.refuse_named_binding(
                    Construct::BindingForm,
                    name.as_str(),
                    clause,
                ));
            }
            handler.extend(self.block(&clause.body));
        }

        let cleanup = self.block(&node.finalbody);
        let origin = self.origin(node);
        vec![self.builder.protected(body, handler, cleanup, origin)]
    }

    /// `with`, which is a body between two calls the source text does not show.
    ///
    /// # No new theory, and two calls the frontend could not previously see
    ///
    /// `with lock:` evaluates `lock`, calls `lock.__enter__()`, runs the body,
    /// and calls `lock.__exit__(...)` - on the normal path *and* on the
    /// exceptional one, which is the entire reason the statement exists. Both
    /// are calls to arbitrary user code, and this frontend already refuses
    /// every call it can see as [`Construct::Call`]. These two are no different
    /// for being implicit, and naming them anything else tells a user their
    /// exception handling was refused when what they need to know is that a
    /// context manager's `__exit__` has no bound.
    ///
    /// `__exit__` goes in the cleanup slot for the same reason `finally` does.
    ///
    /// # What the `__enter__` hole costs the body, and why that is right
    ///
    /// A call is a region, and a region forgets every readable name, so a
    /// counted loop *inside* a `with` loses the endpoint it is counted by. That
    /// is a real loss and it is not one to reach around: the value bound by
    /// `with ... as name` comes from `__enter__`, so a `with cm as n:` over an
    /// integer parameter `n` is exactly the case where reading it afterwards
    /// would be wrong. Every `__enter__` precedes the body, so this is enforced
    /// without a special case for the target.
    ///
    /// The gain is the body being *translated at all*. It used to be one opaque
    /// statement refusal that swallowed everything inside it; now a call in a
    /// `with` body is a named hole at its own position, charged once per
    /// iteration of the loop that runs it.
    fn with_statement(&mut self, node: &ast::StmtWith) -> Vec<StmtId> {
        let origin = self.origin(node);
        // `__exit__` runs after the last statement of the block, and reporting
        // it there is also what keeps it distinguishable from `__enter__`: a
        // consumer joining a hole to the refusal carrying its specifics has only
        // position and construct to join on.
        let closing = self.origin_after(node);
        let mut body = Vec::new();
        let mut cleanup = Vec::new();
        for (position, item) in node.items.iter().enumerate() {
            if !is_name_lookup(&item.context_expr) {
                body.extend(self.refusals_of(&item.context_expr));
            }
            // The `with` line executes, so exactly one node has to carry its
            // step - a statement executes once however many context managers it
            // opens, and `refusals_of` above emits only fragments. The first
            // `__enter__` is that node.
            let extent = if position == 0 {
                Extent::Statement
            } else {
                Extent::Fragment
            };
            body.push(self.builder.unsupported_stmt_with(
                Construct::Call,
                Some(Symbol::from("__enter__")),
                extent,
                origin.clone(),
            ));
            cleanup.push(self.builder.unsupported_stmt_with(
                Construct::Call,
                Some(Symbol::from("__exit__")),
                Extent::Fragment,
                closing.clone(),
            ));
        }
        body.extend(self.block(&node.body));
        vec![self.builder.protected(body, Vec::new(), cleanup, origin)]
    }

    fn assign(&mut self, assign: &ast::StmtAssign) -> Vec<StmtId> {
        let [target] = assign.targets.as_slice() else {
            // `a = b = 0` binds two names; the fragment's assignment binds one.
            let mut statements = self.refusals_of(&assign.value);
            statements.push(self.refuse_targets(assign, assign.targets.iter()));
            return statements;
        };
        let Expr::Name(name) = target else {
            let mut statements = self.refusals_of(&assign.value);
            statements.push(self.refuse_targets(assign, [target]));
            return statements;
        };
        self.bind_expression(name.id.as_str(), target, &assign.value, assign, 0)
    }

    /// Emits `name = value`, splitting a conditional expression into a branch.
    ///
    /// # Why a ternary is a statement here and not an expression
    ///
    /// `x = a if c else b` and the four-line `if` that spells it out are the
    /// same program, and the engine has derived the second since it existed.
    /// What separated them was the shape of this frontend: an expression
    /// translation that could only return an `ExprId` had nowhere to put a
    /// branch, so the ternary became one opaque region - the sole blocker for
    /// 118 stdlib functions across 369 sites.
    ///
    /// Rewriting it into [`landav_its::SourceStmt::If`] reaches the arithmetic
    /// that already exists: the test is charged once whichever way it goes, and
    /// the arms are combined by **maximum** by `TripCount::branching`, because
    /// exactly one of them runs.
    ///
    /// # Why not hoist the arms' refusals instead
    ///
    /// That is the cheaper change and it is permanently loose.
    /// [`Translator::refusals_of`] turns the refusals inside an expression into
    /// a *list* of statements, and the engine **sums** a statement list - right
    /// for `x = f(n) + g(n)`, where both calls happen, and wrong here, where one
    /// does. Arms costing 10 and 3 would be reported at 13 where the truth is
    /// 10: sound, and wrong on every one of those 369 sites for ever.
    ///
    /// # What this does not buy
    ///
    /// The ternary's *value*. `x` afterwards holds one of two numbers and the
    /// fragment has no expression for that - there is no maximum in
    /// [`landav_its::SourceExpr`] and deliberately never will be, since every
    /// variant of it must denote a polynomial. The name is bound inside each
    /// arm, so a later trip count over `x` reads it as a value no arm can vouch
    /// for and holes, which is the honest answer.
    fn bind_expression<T: Ranged>(
        &mut self,
        name: &str,
        target: &Expr,
        value: &Expr,
        node: &T,
        depth: u32,
    ) -> Vec<StmtId> {
        if let Expr::IfExp(ternary) = value
            && depth < MAX_TERNARY_DEPTH
        {
            let cond = self.condition(&ternary.test);
            let then_body = self.bind_expression(name, target, &ternary.body, node, depth + 1);
            let else_body = self.bind_expression(name, target, &ternary.orelse, node, depth + 1);
            let origin = self.origin(ternary);
            return vec![self.builder.if_else(cond, then_body, else_body, origin)];
        }
        if !self.integers.contains(name) {
            return self.refuse_binding(name, target, value);
        }
        let translated = self.expression(value);
        self.bind(name, translated, node)
    }

    fn aug_assign(&mut self, assign: &ast::StmtAugAssign) -> Vec<StmtId> {
        let Expr::Name(name) = assign.target.as_ref() else {
            let mut statements = self.refusals_of(&assign.value);
            statements.push(self.refuse_targets(assign, [assign.target.as_ref()]));
            return statements;
        };
        let Some(op) = arith_of(&assign.op) else {
            // `x //= e` rebinds `x`, and the operator may run user code on
            // whatever `x` holds. The value is not translated here, so its
            // interior speaks for the statement.
            let mut writes = self.target_writes([assign.target.as_ref()]);
            if rebinds_the_frame([assign.value.as_ref()]) {
                writes = Writes::frame();
            } else if let Locals::AtMost(names) = writes.locals() {
                writes = Writes::at_most(names.iter().cloned());
            }
            let origin = self.origin(assign);
            return vec![self.builder.unsupported_stmt_writing(
                refusal_for(&assign.op),
                None,
                Extent::Statement,
                writes,
                origin,
            )];
        };
        if !self.integers.contains(name.id.as_str()) {
            // `x += e` reads `x` as well as writing it, and both are refusals
            // about the same name at the same position - the read cannot be
            // turned into a value, and neither can what the statement leaves
            // behind. They were charged separately, and the value was
            // translated twice on top of that: `m += obj.k` produced
            // `attribute` twice at one column and `non-integer-value` twice at
            // another, five regions for a two-line function. One construct at
            // one position is one region: the report names one fix once, the
            // per-construct ledger counts one occurrence once, and filling the
            // hole adds the read's cost once.
            return self.refuse_binding(name.id.as_str(), assign.target.as_ref(), &assign.value);
        }
        // `x += e` is `x = x + e`. The expansion is a Python fact, and it
        // stays on this side of the boundary.
        let origin = self.origin(assign);
        let read = self.read(name.id.as_str(), assign.target.as_ref());
        let value = self.expression(&assign.value);
        let combined = self.builder.arith(op, read, value, origin);
        self.bind(name.id.as_str(), combined, assign)
    }

    fn ann_assign(&mut self, assign: &ast::StmtAnnAssign) -> Vec<StmtId> {
        let Some(value) = &assign.value else {
            // `x: int` with no value binds nothing.
            return Vec::new();
        };
        let Expr::Name(name) = assign.target.as_ref() else {
            let mut statements = self.refusals_of(value);
            statements.push(self.refuse_targets(assign, [assign.target.as_ref()]));
            return statements;
        };
        if !annotation_is_int(Some(&assign.annotation)) {
            let mut statements = self.refusals_of(value);
            statements.push(self.refuse_named_binding(
                Construct::NonIntegerValue,
                name.id.as_str(),
                assign,
            ));
            return statements;
        }
        self.bind_expression(name.id.as_str(), assign.target.as_ref(), value, assign, 0)
    }

    /// Emits `name = value`.
    ///
    /// The caller has already established that `name` is a proven integer;
    /// [`Translator::refuse_binding`] is the path for when it has not.
    fn bind<T: Ranged>(&mut self, name: &str, value: ExprId, node: &T) -> Vec<StmtId> {
        let origin = self.origin(node);
        vec![self.builder.assign(VarName::new(name), value, origin)]
    }

    /// Refuses a binding whose target is not a proven integer, keeping the
    /// value's own refusals as statements in front of it.
    ///
    /// The variable's value after this statement is unknown, so every later
    /// guard mentioning it would be wrong. That is one refusal; a call on the
    /// right-hand side is another, and both are real. Hoisting the second means
    /// the arena holds no node that the statement tree cannot reach - see
    /// [`Translator::refusals_of`].
    fn refuse_binding(&mut self, name: &str, target: &Expr, value: &Expr) -> Vec<StmtId> {
        let mut statements = self.refusals_of(value);
        statements.push(self.refuse_named_binding(Construct::NonIntegerValue, name, target));
        statements
    }

    /// Refuses a statement that binds exactly `name`, saying so.
    ///
    /// `LAN-100`. The statement stays refused - its value is one this fragment
    /// cannot hold - but what it *does* is known: it rebinds one local and
    /// touches no object, so every other value the engine knew survives it. A
    /// collection parameter's length is a name the source cannot spell, and it
    /// is rebound with the parameter, so it is listed beside it.
    fn refuse_named_binding<T: Ranged>(
        &mut self,
        construct: Construct,
        name: &str,
        node: &T,
    ) -> StmtId {
        let origin = self.origin(node);
        self.builder.unsupported_stmt_writing(
            construct,
            Some(Symbol::from(name)),
            Extent::Statement,
            Writes::only(self.rebound_names(name)),
            origin,
        )
    }

    /// Refuses an assignment whose targets this fragment cannot bind, saying
    /// which locals they reach.
    ///
    /// `LAN-100`. `a = b = 0` rebinds two known names and nothing else; `x.y =
    /// v` rebinds no local and runs a setter; `a, b = pair` rebinds two names
    /// and runs an `__iter__`. Each is narrower than "anything", which is what
    /// the construct answers for all three, and each is what the engine needs
    /// to keep a trip count read three lines earlier. The value's own refusals
    /// are hoisted separately by the caller and carry their own answer.
    fn refuse_targets<'e, T: Ranged>(
        &mut self,
        node: &T,
        targets: impl IntoIterator<Item = &'e Expr>,
    ) -> StmtId {
        let writes = self.target_writes(targets);
        let origin = self.origin(node);
        self.builder.unsupported_stmt_writing(
            Construct::ComplexAssignmentTarget,
            None,
            Extent::Statement,
            writes,
            origin,
        )
    }

    /// The variables a binding of `name` rebinds: the name, and the length
    /// variable standing beside it if `name` is a collection parameter.
    fn rebound_names(&self, name: &str) -> Vec<VarName> {
        let mut names = vec![VarName::new(name)];
        if self.collections.contains(name) {
            names.push(length_var(name));
        }
        names
    }

    /// What binding `targets` may change. See [`Translator::refuse_targets`].
    fn target_writes<'e>(&self, targets: impl IntoIterator<Item = &'e Expr>) -> Writes {
        let mut names: Vec<VarName> = Vec::new();
        let mut mutates = false;
        let mut work: Vec<&Expr> = targets.into_iter().collect();
        while let Some(target) = work.pop() {
            match target {
                Expr::Name(name) => names.extend(self.rebound_names(name.id.as_str())),
                // Unpacking runs an `__iter__` on the value, which is user code.
                Expr::Tuple(tuple) => {
                    mutates = true;
                    work.extend(tuple.elts.iter());
                }
                Expr::List(list) => {
                    mutates = true;
                    work.extend(list.elts.iter());
                }
                Expr::Starred(starred) => {
                    mutates = true;
                    work.push(starred.value.as_ref());
                }
                // A setter and a `__setitem__` run in their own frame and
                // rebind nothing here - unless the object or the index hides a
                // walrus or a call, which the fragment did not translate.
                Expr::Attribute(attribute) => {
                    mutates = true;
                    if rebinds_the_frame([attribute.value.as_ref()]) {
                        return Writes::frame();
                    }
                }
                Expr::Subscript(subscript) => {
                    mutates = true;
                    if rebinds_the_frame([subscript.value.as_ref(), subscript.slice.as_ref()]) {
                        return Writes::frame();
                    }
                }
                // Not a target Python accepts; refuse as widely as possible
                // rather than guess.
                _ => return Writes::frame(),
            }
        }
        if mutates {
            Writes::at_most(names)
        } else {
            Writes::only(names)
        }
    }

    /// Refuses an expression, saying what its untranslated interior may
    /// change. See [`effect_of_untranslated`].
    fn refuse_expr(
        &mut self,
        construct: Construct,
        detail: Option<&str>,
        node: &Expr,
        origin: Origin,
    ) -> ExprId {
        let writes = effect_of_untranslated([node]);
        self.builder
            .unsupported_expr_writing(construct, detail.map(Symbol::from), writes, origin)
    }

    /// The condition counterpart of [`Translator::refuse_expr`], over every
    /// operand the refused condition would have evaluated.
    fn refuse_cond<'e>(
        &mut self,
        construct: Construct,
        detail: Option<&str>,
        roots: impl IntoIterator<Item = &'e Expr>,
        origin: Origin,
    ) -> CondId {
        let writes = effect_of_untranslated(roots);
        self.builder
            .unsupported_cond_writing(construct, detail.map(Symbol::from), writes, origin)
    }

    /// `for x in items:` where `items` is a collection parameter.
    ///
    /// # Why the counter is synthetic rather than the loop's own target
    ///
    /// The target binds an **element**, and an element of a `list` may be a
    /// string. [`landav_its::VarName`] promises a mathematical integer, so the
    /// target may not become one - `integer_names` already dooms it, which is
    /// what makes a read of it in the body refuse.
    ///
    /// But [`landav_its::SourceStmt::ForRange`] needs *some* counter, and the
    /// engine adds it to the values it may read while deriving the body. Giving
    /// it the element's name would mean one name standing for two things: a
    /// counter the engine may read, and an element it may not. They stay apart
    /// here instead. `#` is not legal in a Python identifier, so the synthetic
    /// name cannot collide with the one the source bound.
    fn walk_collection(&mut self, loop_stmt: &ast::StmtFor, collection: &str) -> Vec<StmtId> {
        let origin = self.origin(loop_stmt);
        let start = self.builder.int(0, origin.clone());
        let stop = self.builder.var(length_var(collection), origin.clone());
        let counter = self.walk_counter();

        let body = self.block(&loop_stmt.body);
        let Some(stride) = core::num::NonZeroI64::new(1) else {
            unreachable!("1 is non-zero")
        };
        vec![
            self.builder
                .for_range(counter, RangeSpec::new(start, stop, stride), body, origin),
        ]
    }

    /// `for x in [a, b, c]:` - a display, whose length is in the source.
    ///
    /// The counter is synthetic for the same reason as in
    /// [`Translator::walk_collection`]: the target binds an **element**, and
    /// `for x in [1, 'a']` is legal Python, so a target promoted on the strength
    /// of the elements it happened to have would need element type inference to
    /// stay sound.
    ///
    /// The display's own elements are translated first, as statements in front
    /// of the loop. That is where they belong - the display is built once,
    /// before the first iteration - and it is not optional: `for x in [g(n), 2]`
    /// runs `g` exactly once, and a loop that counted the elements while never
    /// translating them would publish a complete bound for a function that
    /// calls something.
    ///
    /// `exact` comes from [`walked_display`]. Where it is false the endpoint is
    /// a refusal *bounded by* the written element count, which the engine reads
    /// as an upper bound and reports as `O` rather than `Theta`.
    fn walk_display(&mut self, loop_stmt: &ast::StmtFor, length: u64, exact: bool) -> Vec<StmtId> {
        let mut statements = self.refusals_of(&loop_stmt.iter);
        let origin = self.origin(loop_stmt);
        let Ok(length) = i64::try_from(length) else {
            statements.push(self.refuse_stmt(Construct::UnboundedIteration, loop_stmt));
            return statements;
        };

        let start = self.builder.int(0, origin.clone());
        let written = self.builder.int(length, origin.clone());
        let stop = if exact {
            written
        } else {
            self.builder.unsupported_expr_bounded(
                Construct::Collection,
                "set or dict display, whose equal elements collapse",
                written,
                origin.clone(),
            )
        };
        let counter = self.walk_counter();

        let body = self.block(&loop_stmt.body);
        let Some(stride) = core::num::NonZeroI64::new(1) else {
            unreachable!("1 is non-zero")
        };
        statements.push(self.builder.for_range(
            counter,
            RangeSpec::new(start, stop, stride),
            body,
            origin,
        ));
        statements
    }

    /// A fresh synthetic loop counter.
    ///
    /// `#` is not legal in a Python identifier, so one of these can never
    /// collide with a name the source bound. Every counted walk over something
    /// that is not an integer range uses one, for the reason
    /// [`Translator::walk_collection`] gives: the target holds a value, the
    /// counter holds a number, and one name may not stand for both.
    fn walk_counter(&mut self) -> VarName {
        let counter = VarName::new(format!("#walk{}", self.walks));
        self.walks += 1;
        counter
    }

    /// `for r in sorted(records):` - a walk over the result of a
    /// length-preserving call, counted by the length of that call's argument.
    ///
    /// # Why the call's own cost is charged **after** the loop
    ///
    /// This is the one ordering decision in `LAN-99` and it is load bearing, so
    /// here is the argument.
    ///
    /// The call is a region, and a region forgets: `Construct::Call` may rebind
    /// a local, and a collection's length variable is volatile besides
    /// ([`landav_its::SourceProgram::is_volatile`]), so charging it in front of
    /// the loop clears `len(records)` from the set of names the engine may read
    /// and the loop loses the very count this ticket exists to derive.
    ///
    /// Charging it after is not a way round that rule; it is the rule applied to
    /// the right state. `sorted(records)` copies `records` into a list and only
    /// then compares, so **the number of values it yields is fixed before any
    /// effect of the call can be observed** - the trip count is a reading of the
    /// state on entry to the statement, which is exactly what a
    /// [`landav_bound::Bound`] over `len(records)` denotes. The region then
    /// forgets everything downstream of the loop, which is where a mutation
    /// performed by a comparison could first be seen. The total is unchanged
    /// either way: the call runs once, and it is charged once, outside the
    /// multiplication.
    ///
    /// The residual is narrow and worth writing down: the loop **body** is
    /// walked with the entry state, so a body that reads `len(records)` while a
    /// comparison inside `sorted` appended to `records` would read a stale
    /// length. Closing that needs the engine to distinguish "before the
    /// iterable" from "after the iterable" within one statement, which the
    /// statement list it walks cannot express today. `LAN-11`.
    fn walk_length(&mut self, loop_stmt: &ast::StmtFor, walk: &LengthWalk) -> Vec<StmtId> {
        let origin = self.origin(loop_stmt);
        let start = self.builder.int(0, origin.clone());
        let length = self
            .builder
            .var(length_var(&walk.collection), origin.clone());
        // An inexact relation - `set`, whose equal elements collapse - becomes a
        // refusal *bounded by* the argument's length, which the engine reads as
        // an upper bound and reports as `O` rather than `Theta`. The same
        // constructor `walk_display` reaches for, for the same reason, and it is
        // the reason exactness lives in `ResultLength`'s value rather than in a
        // flag beside it.
        let stop = if walk.exact {
            length
        } else {
            self.builder.unsupported_expr_bounded(
                Construct::Collection,
                "a callee whose result collapses equal elements",
                length,
                origin.clone(),
            )
        };
        let counter = self.walk_counter();

        let body = self.block(&loop_stmt.body);
        let Some(stride) = core::num::NonZeroI64::new(1) else {
            unreachable!("1 is non-zero")
        };
        let mut statements = vec![self.builder.for_range(
            counter,
            RangeSpec::new(start, stop, stride),
            body,
            origin,
        )];
        // The call itself: `sorted` is n log n and stays a hole, `enumerate` is
        // constant and is discharged by its signature row. Whichever it is, the
        // callee is named where a reader can find it and an unknown call written
        // inside it - `sorted(expensive(records))` - is named beside it.
        statements.extend(self.refusals_of(&loop_stmt.iter));
        statements
    }

    /// A `range` endpoint, with the cost of producing it deferred to
    /// `trailing`.
    ///
    /// Ordinary endpoints translate exactly as they always did and defer
    /// nothing. The one exception is `len(<a length-preserving call>)`, which
    /// reads as the argument's length variable - so that `LAN-89`'s
    /// `for i in range(len(...))` idiom gets the relation too - and whose
    /// callees are charged after the loop for the reason
    /// [`Translator::walk_length`] gives at length.
    fn endpoint(&mut self, expr: &Expr, trailing: &mut Vec<StmtId>) -> ExprId {
        let Some((walk, measured)) = self.measured_length(expr) else {
            return self.expression(expr);
        };
        let origin = self.origin(expr);
        let length = self
            .builder
            .var(length_var(&walk.collection), origin.clone());
        let reading = if walk.exact {
            length
        } else {
            self.builder.unsupported_expr_bounded(
                Construct::Collection,
                "a callee whose result collapses equal elements",
                length,
                origin,
            )
        };
        trailing.extend(self.refusals_of(measured));
        reading
    }

    /// `len(x)` where `x`'s length is known through a relation rather than
    /// because `x` is a collection parameter.
    ///
    /// Returns the relation and the expression whose cost still has to be
    /// charged. `len(items)` for a bare collection parameter is deliberately
    /// **not** matched here: [`length_of_collection`] already reads it as a
    /// variable, it costs nothing, and there is nothing to charge.
    fn measured_length<'e>(&self, expr: &'e Expr) -> Option<(LengthWalk, &'e Expr)> {
        let Expr::Call(call) = expr else {
            return None;
        };
        let Expr::Name(callee) = call.func.as_ref() else {
            return None;
        };
        if callee.id.as_str() != "len" || !call.keywords.is_empty() {
            return None;
        }
        // A *call* whose length is known, and nothing else. `len(items)` for a
        // bare collection parameter is [`length_of_collection`]'s, which reads
        // it as a variable and charges nothing - translating it here instead
        // would refuse `items` as `non-integer-value` for its value, which is
        // not the question `len` asks.
        let [measured @ Expr::Call(_)] = call.args.as_slice() else {
            return None;
        };
        // `len` itself is matched by name, exactly as `length_of_collection`
        // matches it and on the same trust the `int` annotation already gets.
        // The *inner* callee goes through the pack and its shadowing gate.
        self.walked_length(measured).map(|walk| (walk, measured))
    }

    /// The length relation an iterable's element count is known by, if any.
    ///
    /// # The relation is conditional on the ARGUMENT, never on the callee's name
    ///
    /// `enumerate(x)` yields `len(x)` pairs only when `x` has a length to yield.
    /// So this descends: a bare collection parameter is the base case, a
    /// declared row over an argument that is itself one of these composes, and
    /// anything else answers `None`: a generator, a comprehension, a local, an
    /// unknown callee. A row keyed on the name alone would fire on
    /// `enumerate(stream())` and publish an equality for a count nothing knows.
    ///
    /// Exactness is conjunctive: one `at-most` anywhere in the chain makes the
    /// whole reading an upper bound, because `set` of a `sorted` holds at most
    /// as many values as the original and possibly fewer.
    ///
    /// # Argument 0 is the receiver for a method
    ///
    /// `d.items()` has as many values as `d`, and `len(d.items()) == len(d)`
    /// needs no vocabulary beyond "argument 0". This is where a pack row first
    /// bites on a bare attribute: [`Translator::declaration_for`] refuses to
    /// resolve a method's *cost*, because a row keyed `items` matches any
    /// object's `.items()` and the receiver's class is something this analysis
    /// has never seen. The length claim is admitted where the cost claim is not,
    /// and the difference is the gate rather than the row: the receiver has to
    /// be a collection the caller supplied.
    ///
    /// Iterative rather than recursive: each step descends strictly into a
    /// subexpression, so it terminates, and expression depth is capped at ten
    /// thousand - deep enough that a recursive version would risk the stack.
    fn walked_length(&self, iterable: &Expr) -> Option<LengthWalk> {
        let mut node = iterable;
        let mut exact = true;
        loop {
            if let Some(collection) = walked_collection(node, &self.collections) {
                return Some(LengthWalk { collection, exact });
            }
            let Expr::Call(call) = node else {
                return None;
            };
            // A keyword argument means this is some other spelling of the call
            // than the one the row was written for.
            if !call.keywords.is_empty() {
                return None;
            }
            let (callee, receiver) = match call.func.as_ref() {
                Expr::Name(name) => (name.id.as_str(), None),
                Expr::Attribute(attribute) => {
                    (attribute.attr.as_str(), Some(attribute.value.as_ref()))
                }
                _ => return None,
            };
            let relation = self
                .pack
                .length_relation(callee, |name| self.is_shadowed(name))?;
            let argument = match receiver {
                // A method row's argument 0 is its receiver, and the row was
                // written for the no-argument spelling: `d.items()`, never
                // `d.items(something)`.
                Some(receiver) if call.args.is_empty() => receiver,
                Some(_) => return None,
                None => match call.args.as_slice() {
                    [only] => only,
                    _ => return None,
                },
            };
            exact = exact && relation.is_exact();
            node = argument;
        }
    }

    fn for_loop(&mut self, loop_stmt: &ast::StmtFor) -> Vec<StmtId> {
        if !loop_stmt.orelse.is_empty() {
            return vec![self.refuse_stmt_detailed(
                Construct::ExceptionalControlFlow,
                "for ... else",
                loop_stmt,
            )];
        }
        // How many times a `for` runs is a property of the **iterable**, and no
        // property of the target bears on it: `for a, b in pairs` yields one
        // value per element of `pairs` exactly as `for x in pairs` does, and
        // then takes that element apart. So a tuple, a list or a starred target
        // is counted rather than refused, and it binds nothing - `a` and `b`
        // hold pieces of an element, an element of a `list` may be a string, and
        // [`landav_its::VarName`] promises a mathematical integer. They are in
        // exactly the position `x` is already in for `for x in items`, which
        // `walk_collection` counts through a synthetic `#walk` counter without
        // mentioning the target at all. `integer_names` dooms every name in the
        // tree (`written_names` already recurses through `Tuple`, `List` and
        // `Starred`), so a read of one keeps refusing as `non-integer-value`.
        // `LAN-99`.
        //
        // A subscript or an attribute target - `for a[i] in xs` - is a *write
        // through an object*, which is a different question with a different
        // answer, and it stays refused.
        let target = match loop_stmt.target.as_ref() {
            Expr::Name(name) => Some(name.id.as_str()),
            Expr::Tuple(_) | Expr::List(_) | Expr::Starred(_) => None,
            _ => return vec![self.refuse_stmt(Construct::ComplexAssignmentTarget, loop_stmt)],
        };
        let Some(arguments) = range_arguments(&loop_stmt.iter) else {
            // Walking a collection **parameter** is counted by its length.
            // `LAN-89`.
            if let Some(collection) = walked_collection(&loop_stmt.iter, &self.collections) {
                return self.walk_collection(loop_stmt, &collection);
            }
            // Walking the result of a **length-preserving call** is counted by
            // the length of that call's argument. `LAN-99`.
            if let Some(walk) = self.walked_length(&loop_stmt.iter) {
                return self.walk_length(loop_stmt, &walk);
            }
            // Walking a **display** is counted by how many values it holds,
            // which is written in the source. `LAN-91`.
            if let Some((length, exact)) = walked_display(&loop_stmt.iter) {
                return self.walk_display(loop_stmt, length, exact);
            }
            // Iteration over any other container, a generator, `zip`: all need a
            // size model this fragment does not have. The iterable is still
            // translated, because it is still *evaluated*: a call written in it
            // is a call this program issues, and leaving it out of every arena
            // makes it invisible to the refusal ledger and to the walk alike.
            // Measured before `LAN-99`, `for r in sorted(expensive(records))`
            // named neither callee anywhere.
            let mut statements = self.refusals_of(&loop_stmt.iter);
            statements.push(self.refuse_stmt(Construct::UnboundedIteration, loop_stmt));
            return statements;
        };
        // A counter this pass could not prove integral does **not** refuse the
        // loop. Refusing it here reported `non-integer-value` on the `for` line,
        // naming a variable the user did not write while never naming the
        // `attribute` or `subscript` in the endpoint that actually caused it -
        // so the construct was invisible to the per-construct ledger. Worse, it
        // returned before the body was lowered, so the body and any counted loop
        // inside it left the program altogether.
        //
        // The loop is built instead, with a synthetic counter, exactly as
        // [`Translator::walk_collection`] does and for the same reason: the trip
        // count is a property of the range and not of the target, so it is still
        // arithmetic, while a name the fragment cannot vouch for must not become
        // a `landav_its::VarName` the engine may read. Whatever refused in the
        // endpoint is translated below and charged where it stands, and a read
        // of the target inside the body keeps refusing as it always did.
        let counter = match target {
            Some(name) if self.integers.contains(name) => VarName::new(name),
            _ => self.walk_counter(),
        };

        let origin = self.origin(loop_stmt);
        // The cost of producing an endpoint, charged **after** the loop. See
        // [`Translator::endpoint`], which is the only thing that ever fills it.
        let mut trailing: Vec<StmtId> = Vec::new();
        let (start, stop, step) = match arguments {
            [stop] => {
                let zero = self.builder.int(0, origin.clone());
                (zero, self.endpoint(stop, &mut trailing), 1_i64)
            }
            [start, stop] => (
                self.expression(start),
                self.endpoint(stop, &mut trailing),
                1_i64,
            ),
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
                (
                    self.expression(start),
                    self.endpoint(stop, &mut trailing),
                    literal,
                )
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
        let mut statements = vec![self.builder.for_range(
            counter,
            RangeSpec::new(start, stop, stride),
            body,
            origin,
        )];
        statements.extend(trailing);
        statements
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
        // and if it is not, the refusals it produces become statements **in
        // this body, at this position**. That is the whole of `LAN-87`'s
        // coverage claim: a bare `f(n)` is where the calls in the corpus live,
        // and a refusal charged inside the loop that runs it costs once per
        // iteration rather than once.
        //
        // `Extent::Statement` on the first of them, because here the region
        // really is the statement: unlike `return f(n)` or `x = f(n)`, there is
        // no other node in the arena to charge the step that executing this line
        // costs. Any further refusals are fragments of the same statement, which
        // executes once however many of them it contains.
        self.evaluate(bare.value.as_ref(), 0)
    }

    /// Translates an expression evaluated for its effect, splitting a
    /// conditional expression into a branch.
    ///
    /// The statement counterpart of [`Translator::bind_expression`], and it
    /// exists for the same reason: `f() if c else g()` runs one of two calls,
    /// and a refusal list would charge both. See that method for the argument.
    fn evaluate(&mut self, value: &Expr, depth: u32) -> Vec<StmtId> {
        if let Expr::IfExp(ternary) = value
            && depth < MAX_TERNARY_DEPTH
        {
            let cond = self.condition(&ternary.test);
            let then_body = self.evaluate(&ternary.body, depth + 1);
            let else_body = self.evaluate(&ternary.orelse, depth + 1);
            let origin = self.origin(ternary);
            return vec![self.builder.if_else(cond, then_body, else_body, origin)];
        }
        self.hoisted(Extent::Statement, true, |this| {
            let _ = this.expression(value);
        })
    }

    /// The statements that record what `value` refuses, without leaving any of
    /// its nodes in the program.
    ///
    /// # Why the nodes must not be left behind
    ///
    /// A translated-and-discarded expression is an **orphan**: it sits in the
    /// arena with nothing in the statement tree pointing at it.
    /// [`landav_its::lower`] copes, because it scans the arenas - but a
    /// consumer that derives a *cost* has to walk the control structure, and it
    /// cannot place a node the walk never reaches. It has no sound charge for
    /// one either: charged at the top level, an orphan that really belonged in
    /// a loop body is counted once instead of once per iteration, which
    /// understates.
    ///
    /// So the translation happens against a scratch program that is thrown
    /// away, and every refusal it found is re-emitted here as a statement at
    /// the same position, naming the same construct with the same detail. The
    /// refusal ledger [`landav_its::lower`] produces is unchanged; what changes
    /// is that the node is now somewhere a walk can find it.
    fn refusals_of(&mut self, value: &Expr) -> Vec<StmtId> {
        self.hoisted(Extent::Fragment, false, |this| {
            let _ = this.expression(value);
        })
    }

    /// The statements that record what `value` **costs**, for a position that
    /// reads none of it.
    ///
    /// [`Translator::refusals_of`] with the value question dropped as well as
    /// the nodes. The difference is one construct family - a boolean, a
    /// comparison or a ternary, which have no value the fragment can hold but
    /// whose *cost* is entirely their operands' - and the positions that
    /// qualify are the ones with no value slot to fill: `return a and b` and a
    /// bare `a and b` line. See [`Translator::value_discarded`].
    fn discarded_cost_of(&mut self, value: &Expr) -> Vec<StmtId> {
        self.hoisted(Extent::Fragment, true, |this| {
            let _ = this.expression(value);
        })
    }

    /// Runs `translate` against a scratch program and returns its refusals as
    /// statements. See [`Translator::refusals_of`].
    ///
    /// `first` is the [`Extent`] of the leading refusal, and it is the only one
    /// a caller ever has a choice about. A refusal lifted out of a `return`, an
    /// assignment or an augmented assignment is a **fragment**: the statement it
    /// came from is in the arena beside it and pays its own step. A refusal
    /// lifted out of a bare expression statement is the **statement**, because
    /// nothing else stands for that line. Whichever it is, only the first can be
    /// it - a statement executes once no matter how many unanalysable parts it
    /// has - so the rest are fragments.
    ///
    /// `value_discarded` says whether anything at all reads the expression's
    /// value; see [`Translator::value_discarded`] for why that is a separate
    /// question from translating into a scratch builder.
    fn hoisted(
        &mut self,
        first: Extent,
        value_discarded: bool,
        translate: impl FnOnce(&mut Self),
    ) -> Vec<StmtId> {
        // Every node `translate` builds carries its own position, so this
        // fallback origin is never the one reported.
        let scratch =
            SourceProgramBuilder::new("<discarded>", Origin::new("<discarded>"), Vec::new());
        let kept = std::mem::replace(&mut self.builder, scratch);
        let outer = core::mem::replace(&mut self.discarding, true);
        let outer_value = core::mem::replace(&mut self.value_discarded, value_discarded);
        translate(self);
        self.discarding = outer;
        self.value_discarded = outer_value;
        let scratch = std::mem::replace(&mut self.builder, kept);
        let discarded = scratch.build(Vec::new());

        if discarded.overflowed() {
            // The scratch ran out of arena, so its refusals are short and this
            // program is missing at least one. Refusing beats reporting a
            // truncated ledger.
            self.builder.mark_overflowed();
        }
        if discarded.conceals_a_call() {
            // And so does a call the scratch could not account for. Losing this
            // on the way across would publish exactly the confident under-count
            // `LAN-97` closed, for every value hoisted out of a binding.
            self.builder.mark_concealed_call();
        }
        let refusals: Vec<_> = discarded.unsupported_nodes().collect();
        refusals
            .into_iter()
            .enumerate()
            .map(|(position, node)| {
                let extent = if position == 0 {
                    first
                } else {
                    Extent::Fragment
                };
                // A declared node stays declared on the way across. Losing the
                // declaration here would turn every bare `isinstance(x, int)`
                // line back into a hole, which is where they all are.
                match node.declared() {
                    Some(effect) => self.builder.declared_stmt(
                        node.construct(),
                        node.detail().cloned(),
                        extent,
                        effect,
                        node.origin().clone(),
                    ),
                    // And so does what the node said it may change, for the
                    // same reason: a refused binding that lost its write set
                    // on the way across would forget the frame again.
                    None => self.builder.unsupported_stmt_writing(
                        node.construct(),
                        node.detail().cloned(),
                        extent,
                        node.writes().clone(),
                        node.origin().clone(),
                    ),
                }
            })
            .collect()
    }

    /// What the signature pack declares about this call site, if anything.
    ///
    /// # Four conditions, and every one of them is load bearing
    ///
    /// 1. **Nothing reads the value.** Either the nodes are going to a scratch
    ///    builder ([`Translator::discarding`]) or the call is the subject of a
    ///    truth test ([`Translator::truth_test`]). `x = isinstance(a, b)` is
    ///    neither: it names a value, and this fragment has none for a `bool`, so
    ///    a declared node there would be read as the zero `expr_poly` hands out
    ///    for an `Unsupported` node.
    /// 2. **The callee is a bare name.** `x.append(1)` is a method on an object
    ///    whose class this analysis has never seen; matching *any* object's
    ///    `.decode` against a row keyed `decode` would be a far weaker claim
    ///    than matching a module-level `isinstance` against one keyed
    ///    `isinstance`. The pack's method rows are refusals and stay documentary.
    /// 3. **The name is not bound in this module or this function.** See
    ///    [`bindings_of`].
    /// 4. **Every argument is accounted for.** This is the condition the
    ///    soundness of the whole ticket turns on: an unresolved call *denotes
    ///    omega*, which covers for anything hiding inside it, and resolving the
    ///    call takes that cover away. See [`Self::arguments_are_accounted`].
    ///
    /// # `getattr` is the one arity check, and it is a Python fact
    ///
    /// The pack says what a callee costs; which *spellings* of a call site are
    /// that callee is the frontend's question, because arity and keywords are
    /// language grammar. `getattr(x, name)` is one attribute lookup;
    /// `getattr(x, name, default)` is that plus a caught `AttributeError` and a
    /// third expression, and the row was written for the two-argument form.
    fn declaration_for(&self, call: &ast::ExprCall) -> Option<DeclaredEffect> {
        if !(self.discarding || self.truth_test) {
            return None;
        }
        let Expr::Name(callee) = call.func.as_ref() else {
            return None;
        };
        let callee = callee.id.as_str();
        if callee == "getattr" && (call.args.len() != 2 || !call.keywords.is_empty()) {
            return None;
        }
        let row = self.pack.signature(callee, |name| self.is_shadowed(name))?;
        if !self.arguments_are_accounted(call) {
            return None;
        }
        Some(effect_of(row))
    }

    /// What a call the pack cannot *resolve* may nevertheless change in this
    /// frame. `LAN-103`.
    ///
    /// [`Translator::declaration_for`] answers for a callee whose cost is a
    /// constant; the node stops being a hole. This answers for the rest: a row
    /// that says the callee cannot rebind a local of the caller's frame - which
    /// is every row in the pack, `list` and `sorted` and `split` included -
    /// while saying nothing it can bound about the cost. The call stays a hole
    /// and denotes omega; what it no longer does is forget the frame, so
    /// `directories = list(...)` above `for d in directories_list` no longer
    /// costs that loop its trip count. Measured: 13 of the 42 typed-corpus
    /// loops still uncounted after `LAN-100` were blocked by exactly this.
    ///
    /// # What the row does not cover, and the scan does
    ///
    /// The row speaks for the callee. An operand the fragment did not
    /// translate - the receiver, an argument that reaches no call - may hide a
    /// walrus or a call to a closure over this frame, and the row knows nothing
    /// about that. Those are scanned as any refused expression's interior is
    /// ([`effect_of_untranslated`]); an argument that reaches a call is its own
    /// node with its own answer and is not scanned twice.
    ///
    /// `mutates_arguments` is carried through as the object half of the answer
    /// and is `true` for every row today: `list(x)` runs `x.__iter__`, which is
    /// user code, so a collection's length read on entry still goes.
    fn writes_of_call(&self, call: &ast::ExprCall) -> Writes {
        let Some(row) = self.row_for(call) else {
            return Writes::unstated();
        };
        if row.rebinds_locals {
            return Writes::unstated();
        }
        let untranslated = core::iter::once(call.func.as_ref()).chain(
            call_arguments(call).into_iter().filter(|argument| {
                !reaches_a_call(
                    argument,
                    &self.integers,
                    &self.collections,
                    self.discarding,
                    self.value_discarded,
                )
            }),
        );
        if rebinds_the_frame(untranslated) {
            return Writes::unstated();
        }
        if row.mutates_arguments {
            Writes::at_most([])
        } else {
            Writes::only([])
        }
    }

    /// The pack row for a call site, resolvable or not.
    ///
    /// A bare name matches under `LAN-94`'s rule: not bound in this module or
    /// this function. A method matches under `LAN-99`'s: the receiver is a
    /// parameter annotated with the builtin the row was written for, and -
    /// the part `LAN-99` did not need, because a rebound receiver has no known
    /// length either way - never rebound here, so it still holds what the
    /// caller passed. `obj.copy()` on an unannotated `obj` matches nothing.
    fn row_for(&self, call: &ast::ExprCall) -> Option<&'static Signature> {
        match call.func.as_ref() {
            Expr::Name(callee) => {
                let callee = callee.id.as_str();
                if self.is_shadowed(callee) {
                    return None;
                }
                self.pack.row(callee)
            }
            Expr::Attribute(method) => {
                let Expr::Name(receiver) = method.value.as_ref() else {
                    return None;
                };
                let receiver = receiver.id.as_str();
                let rebound = self
                    .rebound
                    .as_ref()
                    .is_none_or(|names| names.contains(receiver));
                if !self.collections.contains(receiver) || rebound {
                    return None;
                }
                self.pack.row(method.attr.as_str())
            }
            _ => None,
        }
    }

    /// Whether `name` means something in this source other than the builtin.
    ///
    /// A scope whose bindings could not be enumerated answers `true` for every
    /// name, which disqualifies every signature. Failing closed costs coverage
    /// and never soundness.
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed
            .as_ref()
            .is_none_or(|names| names.contains(name))
    }

    /// Whether every argument of `call` either costs nothing or is translated.
    ///
    /// # The cover that resolving a call removes
    ///
    /// `isinstance(x, obj.kind)` reports one `call` region today, and that
    /// region denotes `omega`: it dominates whatever the attribute access costs,
    /// so nothing is lost by not translating `obj.kind`. Declare `isinstance` a
    /// constant and the cover is gone - the function becomes a complete bound
    /// with a `property` getter missing from it, which is a bound below the
    /// truth.
    ///
    /// [`expression_children`] translates exactly the arguments that **reach a
    /// call**, for a measured reason recorded there, and widening that for every
    /// call site is not this ticket's change to make. So the rule here is the
    /// other way round: an argument that is not translated must be one that
    /// costs nothing, and a call site with any other kind of argument keeps its
    /// hole.
    ///
    /// A name and a literal cost nothing; a tuple or list of them costs nothing
    /// either, which is what `isinstance(x, (int, str))` needs. Everything else
    /// costs something or runs user code - an attribute runs a `property`, a
    /// subscript runs a `__getitem__`, an f-string runs `__format__` - and is
    /// refused here rather than assumed free.
    fn arguments_are_accounted(&self, call: &ast::ExprCall) -> bool {
        call_arguments(call).into_iter().all(|argument| {
            costs_nothing(argument)
                || reaches_a_call(
                    argument,
                    &self.integers,
                    &self.collections,
                    self.discarding,
                    self.value_discarded,
                )
        })
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
                self.refuse_cond(Construct::ConditionalExpression, None, [root], origin)
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
                    self.refuse_cond(Construct::ConditionalExpression, None, [node], origin)
                })
            }

            Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => {
                match recall(&unary.operand) {
                    Some(operand) => self.builder.not(operand, origin),
                    None => {
                        self.refuse_cond(Construct::ConditionalExpression, None, [node], origin)
                    }
                }
            }

            Expr::Compare(comparison) => self.comparison(comparison),

            // Everything else in a condition position is a truth test on a
            // value. In Python that means "not equal to zero" -- a language
            // fact, spelled out here so that Core never learns it. If the value
            // is not an integer the translation of it refuses, and the
            // comparison simply carries that refusal.
            other => {
                // Nothing reads this as a number: the comparison below is how
                // truthiness is spelled, and `landav_its::lower` answers a
                // comparison over a declared node with "either branch". So a
                // signature may be resolved here even though the nodes are kept
                // - which is what makes `if isinstance(x, int):` the shape the
                // corpus actually writes rather than a shape only a bare
                // statement gets. See [`Translator::truth_test`].
                let outer = core::mem::replace(&mut self.truth_test, true);
                let value = self.expression(other);
                self.truth_test = outer;
                let zero = self.builder.int(0, origin.clone());
                self.builder.compare(CompareOp::Ne, value, zero, origin)
            }
        }
    }

    fn comparison(&mut self, comparison: &ast::ExprCompare) -> CondId {
        let origin = self.origin(comparison);

        // Decide **before** translating an operand. `in`, `not in`, `is` and
        // `is not` are membership and identity, both of which need a model this
        // fragment does not have - and the refusal that stands in for them is a
        // fresh node that references neither side. An operand translated first
        // and then abandoned is an `Unsupported` node in the arena with nothing
        // pointing at it, which `Walk::reconciled` answers with `Unknown` for
        // the entire function rather than a region for the comparison. `LAN-90`.
        //
        // Only the *refused* operators need this. An operand of a comparison
        // that is understood stays referenced by the `compare` node below, so it
        // is reachable and gets charged where it stands.
        if let Some(refused) = comparison
            .ops
            .iter()
            .find(|candidate| compare_of(candidate).is_none())
        {
            let construct = match refused {
                ast::CmpOp::In | ast::CmpOp::NotIn => Construct::Collection,
                _ => Construct::NonIntegerValue,
            };
            return self.refuse_cond(
                construct,
                Some("membership or identity comparison"),
                compare_operands(comparison),
                origin,
            );
        }

        let mut left = self.expression(&comparison.left);
        let mut combined: Option<CondId> = None;

        for (op, right_expr) in comparison.ops.iter().zip(comparison.comparators.iter()) {
            let right = self.expression(right_expr);
            let Some(operator) = compare_of(op) else {
                // Unreachable: every operator was checked above. Kept as a
                // refusal rather than a panic - a frontend that grows an
                // operator and forgets the guard must still refuse, not abort.
                let construct = match op {
                    ast::CmpOp::In | ast::CmpOp::NotIn => Construct::Collection,
                    _ => Construct::NonIntegerValue,
                };
                return self.refuse_cond(
                    construct,
                    Some("membership or identity comparison"),
                    compare_operands(comparison),
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
            self.refuse_cond(
                Construct::ConditionalExpression,
                None,
                compare_operands(comparison),
                origin,
            )
        })
    }

    // -- expressions --------------------------------------------------------

    /// Translates an expression, worklist-driven.
    fn expression(&mut self, root: &Expr) -> ExprId {
        let discarding = self.discarding;
        let value_discarded = self.value_discarded;
        let ordered = postorder(root, |expr| {
            expression_children(
                expr,
                &self.integers,
                &self.collections,
                discarding,
                value_discarded,
            )
        });
        // `LAN-97`: a call this traversal did not reach gets no node, so it is
        // in no ledger and no bound, and a projection counting calls would
        // report a *confident* number below the truth. The program says so
        // once, here, rather than at each of the arms that can do it.
        if conceals_a_call(root, &ordered) {
            self.builder.mark_concealed_call();
        }
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
                self.refuse_expr(Construct::NonIntegerValue, None, root, origin)
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
                    Err(_) => self.refuse_expr(Construct::ArithmeticOverflow, None, node, origin),
                },
                // `True` is `1` and `False` is `0`, exactly, in Python.
                Constant::Bool(flag) => self.builder.int(i64::from(*flag), origin),
                // A string, a bytes or a tuple constant. Evaluated for its
                // effect it has none - nothing is written and no name changes
                // meaning - so in a discarded position it is not a refusal at
                // all. See [`Translator::discarding`].
                Constant::Str(_) | Constant::Bytes(_) | Constant::Tuple(_) => {
                    if self.discarding {
                        return self.builder.int(0, origin);
                    }
                    self.refuse_expr(Construct::Collection, None, node, origin)
                }
                _ => self.refuse_expr(Construct::NonIntegerValue, None, node, origin),
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
                            None => {
                                self.refuse_expr(Construct::NonPolynomialPower, None, node, origin)
                            }
                        };
                    }
                    // `//`, `%`, `>>` and `<<` are not polynomials, but three
                    // of them are dominated by their left operand and the
                    // fourth is a multiplication. See [`Approximated`].
                    if let Some(approximation) =
                        approximation_of(binary, &self.integers, &self.collections)
                        && let Some(left) = recall(&binary.left)
                    {
                        return match approximation {
                            Approximated::Scaled(factor) => {
                                let scale = self.builder.int(factor, origin.clone());
                                self.builder.arith(ArithOp::Mul, left, scale, origin)
                            }
                            // The refusal keeps pointing at the dividend, so
                            // the engine may read its magnitude and the ITS
                            // still refuses to build a system it cannot model.
                            Approximated::ByDividend => self.builder.unsupported_expr_bounded(
                                refusal_for(&binary.op),
                                spelling_of(&binary.op),
                                left,
                                origin,
                            ),
                        };
                    }
                    return self.refuse_expr(refusal_for(&binary.op), None, node, origin);
                };
                match (recall(&binary.left), recall(&binary.right)) {
                    (Some(left), Some(right)) => self.builder.arith(op, left, right, origin),
                    _ => self.refuse_expr(Construct::NonIntegerValue, None, node, origin),
                }
            }

            Expr::UnaryOp(unary) => match unary.op {
                ast::UnaryOp::USub => match recall(&unary.operand) {
                    Some(operand) => self.builder.neg(operand, origin),
                    None => self.refuse_expr(Construct::NonIntegerValue, None, node, origin),
                },
                ast::UnaryOp::UAdd => recall(&unary.operand).unwrap_or_else(|| {
                    self.refuse_expr(Construct::NonIntegerValue, None, node, origin)
                }),
                // `~n` is a bitwise operator whose result the fragment has no
                // rule for, and it stays refused wherever it is written. It is
                // the arm next door on purpose: `Not` and `Invert` are one
                // character apart in Python and one variant apart here, and the
                // widening below must not reach this one.
                ast::UnaryOp::Invert => {
                    self.refuse_expr(Construct::BitwiseOperator, None, node, origin)
                }
                // `not x`, in a position that reads no value from it. The
                // fourth member of the family below - see the `IfExp | BoolOp`
                // arm for the argument, which is identical and if anything
                // easier here: `not` has one operand, evaluates it exactly once
                // and never short-circuits, so charging its operand is the
                // truth rather than an upper bound. `build_condition` has
                // always handled `Not` in full, so nothing new is being taught
                // about negation; what changes is that a `return not f(n)` now
                // names `f` instead of blaming the negation.
                //
                // Value position stays refused, for the report rather than for
                // soundness: `x = not n` condemns `x` as `non-integer-value`
                // either way - `is_integer_expr` has no `Not` arm and must not
                // gain one - and "`x` is not a proven integer" without "because
                // of the negation" names a symptom and withholds the cause. See
                // [`Translator::value_discarded`].
                ast::UnaryOp::Not => {
                    if self.value_discarded {
                        return self.builder.int(0, origin);
                    }
                    self.refuse_expr(Construct::ConditionalExpression, None, node, origin)
                }
            },

            Expr::Call(call) => {
                // `len(items)` for a collection **parameter** is the one call
                // this fragment can read, because its value is a natural number
                // the caller already knows. Everything else is a region.
                if let Some(name) = length_of_collection(call, &self.collections) {
                    return self.builder.var(length_var(&name), origin);
                }
                let detail = match call.func.as_ref() {
                    Expr::Name(name) => name.id.to_string(),
                    Expr::Attribute(attribute) => attribute.attr.to_string(),
                    _ => "call".to_owned(),
                };
                // The arguments are evaluated whether or not the callee can be
                // read, so a call among them is a call this program issues.
                // Charging them is what makes `fetch(g(n))` two regions rather
                // than one; *referencing* them is what keeps the inner one from
                // being an orphan in the position that is not hoisted through a
                // scratch builder - a condition, where `Walk::reconciled` would
                // otherwise fail the whole function closed to `Unknown`. See
                // [`landav_its::SourceExpr::Unsupported`]'s `evaluates`.
                //
                // `recall` is what keeps this in step with
                // `expression_children`, which descends into some arguments and
                // not others: only a translated argument is in `built`, so this
                // references exactly what exists and cannot name a node that was
                // never made or leave one that was.
                let evaluated: Vec<_> = call_arguments(call)
                    .into_iter()
                    .filter_map(recall)
                    .collect();
                // A callee the signature pack accounts for. `LAN-94`: the node
                // is still an `Unsupported` node - this fragment has no value
                // for a `bool` - but it is no longer a *hole*, so it costs a
                // constant and the loop below the guard keeps its trip count.
                if let Some(effect) = self.declaration_for(call) {
                    return self.builder.declared_expr(
                        Construct::Call,
                        detail,
                        evaluated,
                        effect,
                        origin,
                    );
                }
                // A callee the pack knows cannot rebind a local, at a cost it
                // cannot bound. `LAN-103`: the call stays a hole and the loop
                // below it keeps its trip count.
                let writes = self.writes_of_call(call);
                self.builder.unsupported_expr_evaluating_writing(
                    Construct::Call,
                    detail,
                    evaluated,
                    writes,
                    origin,
                )
            }
            Expr::Attribute(attribute) => self.refuse_expr(
                Construct::Attribute,
                Some(attribute.attr.as_str()),
                node,
                origin,
            ),
            Expr::Subscript(_) | Expr::Slice(_) | Expr::Starred(_) => {
                self.refuse_expr(Construct::Subscript, None, node, origin)
            }
            // A display, and the f-string that shares its shape. Building one
            // costs no source step, so in a discarded position it is free - and
            // its elements have been translated already (see
            // `expression_children`), so a call hidden inside one is named and
            // placed where it stands rather than vanishing with the container.
            Expr::List(_)
            | Expr::Tuple(_)
            | Expr::Set(_)
            | Expr::Dict(_)
            | Expr::JoinedStr(_)
            | Expr::FormattedValue(_) => {
                if self.discarding {
                    return self.builder.int(0, origin);
                }
                self.refuse_expr(Construct::Collection, None, node, origin)
            }
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::GeneratorExp(_) => {
                self.refuse_expr(Construct::Comprehension, None, node, origin)
            }
            Expr::Lambda(_) => self.refuse_expr(Construct::Declaration, None, node, origin),
            Expr::Await(_) | Expr::Yield(_) | Expr::YieldFrom(_) => {
                self.refuse_expr(Construct::Coroutine, None, node, origin)
            }
            Expr::NamedExpr(_) => self.refuse_expr(Construct::BindingForm, None, node, origin),
            // A ternary and an `and`/`or` chain, in a position that reads no
            // value from them. Neither costs a source step of its own and
            // neither can bind anything, so what is left is their operands -
            // already translated by `expression_children`, so a call inside one
            // is named where it stands instead of vanishing with the container.
            //
            // Charging **every** operand is an upper bound rather than the
            // truth, because `and` may skip its right-hand side and a ternary
            // runs one arm of two. Everything the operands contribute here is a
            // refusal, so the over-charge can only ever be a hole that denotes
            // `omega` too many - never a false `Theta`, since a function with a
            // hole is `Partial` and `exact_elsewhere` speaks only for the
            // arithmetic outside the holes, which is untouched.
            //
            // In value position all three stay refused; see
            // [`Translator::value_discarded`].
            Expr::IfExp(_) | Expr::BoolOp(_) => {
                if self.value_discarded {
                    return self.builder.int(0, origin);
                }
                self.refuse_expr(Construct::ConditionalExpression, None, node, origin)
            }
            // A comparison, with the same argument and one exception. `in`,
            // `not in`, `is` and `is not` are refused whatever position they
            // stand in: `x in items` runs `__contains__`, which is arbitrary
            // user code with an arbitrary cost, exactly as `x.y` runs a
            // `property`. `comparison` already refuses them under these names
            // in condition position, and one operator answers to one construct
            // wherever it is written.
            Expr::Compare(comparison) => {
                if self.value_discarded {
                    let Some(refused) = membership_or_identity(comparison) else {
                        return self.builder.int(0, origin);
                    };
                    return self.refuse_expr(
                        refused,
                        Some("membership or identity comparison"),
                        node,
                        origin,
                    );
                }
                self.refuse_expr(Construct::ConditionalExpression, None, node, origin)
            }
        }
    }

    /// Reads a variable, or refuses if it is not a proven integer.
    fn read<T: Ranged>(&mut self, name: &str, node: &T) -> ExprId {
        let origin = self.origin(node);
        if self.integers.contains(name) {
            return self.builder.var(VarName::new(name), origin);
        }
        // A name read has no interior and no effect: it is a value this
        // fragment cannot hold, and nothing else. `LAN-100`.
        self.builder.unsupported_expr_writing(
            Construct::NonIntegerValue,
            Some(Symbol::from(name)),
            Writes::nothing(),
            origin,
        )
    }
}

/// Whether evaluating `expr` is a bare name lookup, or a tuple of them.
///
/// # Why these are the expressions worth *not* translating
///
/// `except ValueError:`, `except (ValueError, TypeError):` and
/// `raise StopIteration` name a class and nothing more. The lookup runs no user
/// code, costs no source step, and its value is never bound to anything this
/// analysis reads - so there is nothing to charge and nothing to refuse.
///
/// [`Translator::read`] would refuse each name as `non-integer-value` all the
/// same, because it answers the *value* question and the value is what is not
/// wanted here. That refusal is a named hole in the report, so translating
/// these would put a spurious hole on nearly every `try` in the corpus and tell
/// the user to go and look at the word `ValueError`.
///
/// Anything else is translated as usual. `except self.errors:` runs a
/// `property`; `raise ValueError(explain(n))` runs a call. Both have unknown
/// costs and both must be named where they stand.
fn is_name_lookup(expr: &Expr) -> bool {
    match expr {
        Expr::Name(_) => true,
        Expr::Tuple(tuple) => tuple.elts.iter().all(is_name_lookup),
        _ => false,
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

/// The construct a comparison refuses under, if any of its operators is
/// membership or identity.
///
/// `None` means every operator is one the fragment understands. Deliberately a
/// free function rather than a method on `Translator::comparison`, which makes
/// the same decision for a **condition**: the two positions build different
/// kinds of node, and the one thing they must agree on is which operators are
/// refused and under what name.
fn membership_or_identity(comparison: &ast::ExprCompare) -> Option<Construct> {
    comparison
        .ops
        .iter()
        .find(|candidate| compare_of(candidate).is_none())
        .map(|refused| match refused {
            ast::CmpOp::In | ast::CmpOp::NotIn => Construct::Collection,
            _ => Construct::NonIntegerValue,
        })
}

/// The children of an expression that the fragment translates.
///
/// Almost only the forms that survive into the fragment have children here. A
/// refused form has none, because it becomes one `Unsupported` node whose
/// interior is never inspected -- which is what keeps a refused comprehension
/// from producing a refusal per node inside it.
///
/// The one exception is a refused **call**, whose arguments are evaluated
/// before it whatever the callee turns out to be. It is an exception because
/// the call's node *references* what is translated under it, so nothing is
/// orphaned; see the arm below for what is descended into and what is not.
fn expression_children<'e>(
    expr: &'e Expr,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
    discarding: bool,
    value_discarded: bool,
) -> Vec<&'e Expr> {
    // A refused call's arguments. The call itself stays refused in every
    // position - nothing here widens the fragment - but its arguments are
    // evaluated before it whatever it turns out to be, so a call written among
    // them is a call this program issues and has to be named where it stands.
    // Leaving them untranslated is how `fetch(g(n))` came to be byte-for-byte
    // the report of `fetch(n)`: `g` was in no arena, no ledger and no bound,
    // and `Walk::reconciled` cannot notice a node that was never built.
    //
    // This is the only arm whose parent is a *refused* form, and it is sound
    // only because `build_expression`'s `Expr::Call` arm points the refusal at
    // whatever this returns, through `SourceExpr::Unsupported`'s `evaluates`.
    // Translating without referencing leaves the orphan `LAN-90` closed;
    // referencing without translating leaves a dangling identifier.
    //
    // # Why only the arguments that reach a call
    //
    // Because an argument's **value** is read by nothing here - the call's own
    // node stands for the result - and its **cost** is the only question left.
    // A name lookup and a literal cost nothing, so translating them buys no
    // region and volunteers an answer to the question nobody asked: measured
    // over `/usr/lib/python3.12`, descending into every argument moves 488
    // functions out of "blocked solely by a call", 440 of them onto
    // `non-integer-value` for an argument whose value is never read. Naming a
    // construct that is not the obstacle sends the reader to the wrong line,
    // which is the same argument `negated_expressions.rs` makes.
    //
    // An argument that reaches an *attribute* or a *subscript* is a real cost
    // this still does not name - `x.y` runs a `property` - and that is a
    // deliberate omission with a measurement attached (148 and 31 functions),
    // not an oversight: the enclosing call's region denotes `omega` and
    // dominates it exactly as it did before, so nothing here is unsound. It is
    // a separate widening, to be argued for and measured on its own.
    if let Expr::Call(call) = expr
        && length_of_collection(call, collections).is_none()
    {
        return call_arguments(call)
            .into_iter()
            .filter(|argument| {
                reaches_a_call(
                    argument,
                    candidates,
                    collections,
                    discarding,
                    value_discarded,
                )
            })
            .collect();
    }
    translated_children(expr, candidates, collections, discarding, value_discarded)
}

/// The expressions a call evaluates before the call itself.
///
/// Positional and keyword arguments alike: `fetch(key=g(n))` runs `g` exactly
/// as `fetch(g(n))` does, and the two live in different lists in the AST.
///
/// Deliberately **not** the callee. `inspect.ismodule(x)` evaluates
/// `inspect.ismodule` - an attribute access, which runs a `property` and so
/// costs something - and naming that is a real improvement, but it is a
/// separate widening with its own measurement: it renames functions the corpus
/// counts as blocked by a call, and this change is not the place to do that
/// quietly.
fn call_arguments(call: &ast::ExprCall) -> Vec<&Expr> {
    call.args
        .iter()
        .chain(call.keywords.iter().map(|keyword| &keyword.value))
        .collect()
}

/// What evaluating expressions this fragment did **not** translate may change
/// in the frame they stand in.
///
/// `LAN-100`. A refused expression becomes one `Unsupported` node whose
/// interior is never inspected - which is what keeps a refused comprehension
/// from producing a refusal per node inside it - so the interior has to be
/// scanned *here*, once, for the two things that decide what the engine may
/// keep reading across the node:
///
/// * a **call**, a **walrus**, an `await` or a `yield` may rebind a local of
///   this frame - a call through a closure over it, a walrus directly - and
///   the answer is the widest one, [`Writes::frame`]. It outranks the
///   construct: `x[(n := 5)]` is a subscript, and a subscript's own answer is
///   "nothing";
/// * anything else runs at most user code in **its own** frame - a `property`,
///   a `__getitem__`, an `__add__` on two objects - which cannot rebind a name
///   here but may mutate an object, so a length read on entry is gone:
///   [`Writes::at_most`] of no names;
/// * a name, a literal, and a tuple or list of those do nothing at all:
///   [`Writes::nothing`].
///
/// A lambda's body runs later, in its own frame, and is not scanned; its
/// defaults are evaluated now and are. A comprehension's walrus binds in the
/// *enclosing* scope, which is exactly why the scan does descend into one.
fn effect_of_untranslated<'e>(roots: impl IntoIterator<Item = &'e Expr>) -> Writes {
    let mut pure = true;
    let mut work: Vec<&Expr> = roots.into_iter().collect();
    while let Some(node) = work.pop() {
        match node {
            Expr::Call(_)
            | Expr::NamedExpr(_)
            | Expr::Await(_)
            | Expr::Yield(_)
            | Expr::YieldFrom(_) => return Writes::frame(),
            Expr::Name(_) | Expr::Constant(_) => {}
            Expr::Tuple(tuple) => work.extend(tuple.elts.iter()),
            Expr::List(list) => work.extend(list.elts.iter()),
            Expr::Lambda(lambda) => {
                pure = false;
                work.extend(lambda_defaults(&lambda.args));
            }
            other => {
                pure = false;
                work.extend(interior_of(other));
            }
        }
    }
    if pure {
        Writes::nothing()
    } else {
        Writes::at_most([])
    }
}

/// Whether [`effect_of_untranslated`] finds something that may rebind a local
/// of this frame among `roots`.
fn rebinds_the_frame<'e>(roots: impl IntoIterator<Item = &'e Expr>) -> bool {
    matches!(effect_of_untranslated(roots).locals(), Locals::Any)
}

/// The operands a comparison evaluates, left to right.
fn compare_operands(comparison: &ast::ExprCompare) -> Vec<&Expr> {
    core::iter::once(comparison.left.as_ref())
        .chain(comparison.comparators.iter())
        .collect()
}

/// The default values a lambda evaluates when it is *defined*.
fn lambda_defaults(args: &ast::Arguments) -> Vec<&Expr> {
    args.posonlyargs
        .iter()
        .chain(args.args.iter())
        .chain(args.kwonlyargs.iter())
        .filter_map(|parameter| parameter.default.as_deref())
        .collect()
}

/// Every direct sub-expression of `expr`, whatever its kind.
///
/// The exhaustive walk [`effect_of_untranslated`] needs, and deliberately
/// separate from [`expression_children`], which lists what the *fragment*
/// translates and must stay that way.
fn interior_of(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::BoolOp(boolean) => boolean.values.iter().collect(),
        Expr::NamedExpr(named) => vec![named.target.as_ref(), named.value.as_ref()],
        Expr::BinOp(binary) => vec![binary.left.as_ref(), binary.right.as_ref()],
        Expr::UnaryOp(unary) => vec![unary.operand.as_ref()],
        Expr::Lambda(lambda) => lambda_defaults(&lambda.args),
        Expr::IfExp(ternary) => vec![
            ternary.test.as_ref(),
            ternary.body.as_ref(),
            ternary.orelse.as_ref(),
        ],
        Expr::Dict(dict) => dict
            .keys
            .iter()
            .flatten()
            .chain(dict.values.iter())
            .collect(),
        Expr::Set(set) => set.elts.iter().collect(),
        Expr::ListComp(comprehension) => core::iter::once(comprehension.elt.as_ref())
            .chain(generator_parts(&comprehension.generators))
            .collect(),
        Expr::SetComp(comprehension) => core::iter::once(comprehension.elt.as_ref())
            .chain(generator_parts(&comprehension.generators))
            .collect(),
        Expr::GeneratorExp(comprehension) => core::iter::once(comprehension.elt.as_ref())
            .chain(generator_parts(&comprehension.generators))
            .collect(),
        Expr::DictComp(comprehension) => [comprehension.key.as_ref(), comprehension.value.as_ref()]
            .into_iter()
            .chain(generator_parts(&comprehension.generators))
            .collect(),
        Expr::Await(awaited) => vec![awaited.value.as_ref()],
        Expr::Yield(yielded) => yielded.value.as_deref().into_iter().collect(),
        Expr::YieldFrom(yielded) => vec![yielded.value.as_ref()],
        Expr::Compare(comparison) => compare_operands(comparison),
        Expr::Call(call) => core::iter::once(call.func.as_ref())
            .chain(call_arguments(call))
            .collect(),
        Expr::FormattedValue(formatted) => core::iter::once(formatted.value.as_ref())
            .chain(formatted.format_spec.as_deref())
            .collect(),
        Expr::JoinedStr(joined) => joined.values.iter().collect(),
        Expr::Constant(_) | Expr::Name(_) => Vec::new(),
        Expr::Attribute(attribute) => vec![attribute.value.as_ref()],
        Expr::Subscript(subscript) => vec![subscript.value.as_ref(), subscript.slice.as_ref()],
        Expr::Starred(starred) => vec![starred.value.as_ref()],
        Expr::List(list) => list.elts.iter().collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().collect(),
        Expr::Slice(slice) => [
            slice.lower.as_deref(),
            slice.upper.as_deref(),
            slice.step.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

/// The expressions a comprehension's `for` clauses evaluate: each target,
/// iterable and filter.
fn generator_parts(generators: &[ast::Comprehension]) -> impl Iterator<Item = &Expr> {
    generators.iter().flat_map(|clause| {
        [&clause.target, &clause.iter]
            .into_iter()
            .chain(clause.ifs.iter())
    })
}

/// Whether a call inside `root` is one the traversal that translated it never
/// reached, and which therefore has no node in the program.
///
/// # Why this is asked centrally rather than at each refusal
///
/// [`expression_children`] descends into what the *fragment* translates, and a
/// refused form is deliberately one node whose interior is left alone - that is
/// what keeps a refused comprehension from producing a refusal per node inside
/// it, and it is sound for a **cost**, because the enclosing region denotes
/// `omega` and dominates whatever is inside.
///
/// It is not sound for a **count of calls**, and `LAN-96` closed the largest
/// case by translating a refused call's arguments where one of them reaches a
/// call. What was left is a list of containers - a callee, a refused binary
/// operator's operands, a subscript index - each its own widening with its own
/// blame cost, and a count that was wrong in the meantime. Asking the question
/// here answers it for every container at once, including the ones nobody has
/// enumerated yet: whatever the traversal did not reach, it says so.
///
/// # What counts as reached
///
/// Node identity, not shape. A call the traversal visited has something in the
/// arena standing for it - a `call` region, a declared node, or the variable
/// `len(items)` becomes - and is accounted for. Anything else is not.
///
/// A **lambda body** is deliberately not walked: [`interior_of`] yields a
/// lambda's defaults, which are evaluated where it is written, and not its
/// body, which runs when it is called. A call there is not one this statement
/// issues.
fn conceals_a_call(root: &Expr, translated: &[&Expr]) -> bool {
    let reached: BTreeSet<usize> = translated
        .iter()
        .map(|node| std::ptr::from_ref(*node) as usize)
        .collect();
    let mut work = vec![root];
    while let Some(node) = work.pop() {
        if matches!(node, Expr::Call(_)) && !reached.contains(&(std::ptr::from_ref(node) as usize))
        {
            return true;
        }
        work.extend(interior_of(node));
    }
    false
}
/// Whether evaluating `expr` costs nothing at all.
///
/// A name lookup and a literal are free, and a display of free things is free -
/// building one costs no source step, which is the same judgement
/// `build_expression` already makes for a display in a discarded position.
///
/// Deliberately a short whitelist rather than a blacklist. An attribute runs a
/// `property`, a subscript runs a `__getitem__`, a comprehension runs a loop, an
/// f-string runs `__format__`, and `a + b` on two lists runs `__add__`: the set
/// of Python expressions that genuinely cost nothing is small, and guessing
/// wrong in the other direction publishes a bound the program exceeds. See
/// [`Translator::arguments_are_accounted`], its only caller.
fn costs_nothing(expr: &Expr) -> bool {
    let mut work = vec![expr];
    while let Some(node) = work.pop() {
        match node {
            Expr::Name(_) | Expr::Constant(_) => {}
            Expr::Tuple(tuple) => work.extend(tuple.elts.iter()),
            Expr::List(list) => work.extend(list.elts.iter()),
            _ => return false,
        }
    }
    true
}

/// Whether translating `root` would produce at least one `call` region.
///
/// The filter on [`expression_children`]'s call arm, and it is written against
/// [`translated_children`] rather than against [`expression_children`] for two
/// reasons. It answers exactly the question asked - *this* traversal reaches a
/// call, so `table[g(n)]` is `false` because the subscript has no children and
/// the descent would stop there having named a subscript and no call - and it
/// terminates without recursion, which matters: expression depth is capped at
/// ten thousand, and a version of this that recursed through the call arm would
/// nest one stack frame per `f(f(f(...)))` and abort.
///
/// The one call the fragment accepts, `len(items)` over a collection
/// parameter, is not a call region: it becomes a variable read.
fn reaches_a_call(
    root: &Expr,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
    discarding: bool,
    value_discarded: bool,
) -> bool {
    let mut work = vec![root];
    while let Some(node) = work.pop() {
        if let Expr::Call(call) = node
            && length_of_collection(call, collections).is_none()
        {
            return true;
        }
        work.extend(translated_children(
            node,
            candidates,
            collections,
            discarding,
            value_discarded,
        ));
    }
    false
}

/// The children of an expression that survive into the fragment, ignoring the
/// arguments of a refused call. See [`expression_children`], its only caller
/// besides [`reaches_a_call`].
fn translated_children<'e>(
    expr: &'e Expr,
    candidates: &BTreeSet<String>,
    collections: &BTreeSet<String>,
    discarding: bool,
    value_discarded: bool,
) -> Vec<&'e Expr> {
    // The operands of a boolean, a comparison or a ternary, where the value of
    // the whole is read by nothing and `build_expression` therefore accepts it.
    // This arm is not optional precision: accepting a container while leaving
    // its interior untranslated is how a call disappears from a program with no
    // hole anywhere, and `Walk::reconciled` cannot catch it because a node that
    // was never built is not in the arena to reconcile. `return f(n) or g(n)`
    // must produce two `call` regions, and it is this that produces them.
    //
    // A comparison that `membership_or_identity` refuses gets none, in step
    // with every other refused form: its `Unsupported` node references nothing,
    // so a refusal among its operands would be an orphan (`LAN-90`).
    if value_discarded {
        let operands: Option<Vec<&'e Expr>> = match expr {
            Expr::BoolOp(boolean) => Some(boolean.values.iter().collect()),
            Expr::IfExp(ternary) => Some(vec![
                ternary.test.as_ref(),
                ternary.body.as_ref(),
                ternary.orelse.as_ref(),
            ]),
            Expr::Compare(comparison) if membership_or_identity(comparison).is_none() => Some(
                core::iter::once(comparison.left.as_ref())
                    .chain(comparison.comparators.iter())
                    .collect(),
            ),
            // The operand of a negation, for the same reason and under the same
            // obligation: `build_expression` accepts `not` here, so `return not
            // f(n)` would publish a complete bound that omits a call unless the
            // operand is translated in the same change. `~n` is deliberately
            // absent - it stays a `bitwise-operator` region, and a region's
            // interior is not translated.
            Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => {
                Some(vec![unary.operand.as_ref()])
            }
            _ => None,
        };
        if let Some(operands) = operands {
            return operands;
        }
    }
    // A display's elements, and only where the display itself is not going to
    // become a refusal. In a **value** position the container refuses and
    // references nothing, so translating its elements would leave every refusal
    // among them pointing at nothing - the orphan shape `LAN-90` fixed. In a
    // **discarded** position the container is free, so the elements are the only
    // thing left that can refuse, and not translating them would publish a
    // complete bound for a function that calls something. Of the 23 stdlib
    // functions blocked solely by `collection`, 22 hide a call, an attribute or
    // a subscript inside the display.
    if discarding {
        let elements: Option<Vec<&'e Expr>> = match expr {
            Expr::List(list) => Some(list.elts.iter().collect()),
            Expr::Tuple(tuple) => Some(tuple.elts.iter().collect()),
            Expr::Set(set) => Some(set.elts.iter().collect()),
            Expr::Dict(dict) => Some(
                dict.keys
                    .iter()
                    .flatten()
                    .chain(dict.values.iter())
                    .collect(),
            ),
            Expr::JoinedStr(joined) => Some(joined.values.iter().collect()),
            Expr::FormattedValue(formatted) => Some(
                core::iter::once(formatted.value.as_ref())
                    .chain(formatted.format_spec.as_deref())
                    .collect(),
            ),
            _ => None,
        };
        if let Some(elements) = elements {
            return elements;
        }
    }
    match expr {
        Expr::BinOp(binary) => match binary.op {
            ast::Operator::Add | ast::Operator::Sub | ast::Operator::Mult => {
                vec![&binary.left, &binary.right]
            }
            // Only the base: the exponent must be a literal, read directly.
            ast::Operator::Pow => vec![&binary.left],
            // An approximated operator keeps its **left** operand: the node
            // built for it references that operand, so it must exist. The right
            // one is deliberately absent - `approximation_of` has already proved
            // it a plain integer expression, so nothing that could cost or
            // assign anything is being dropped. Adding an arm here whenever
            // `build_expression` gains one is not optional: a form with no
            // children has its interior left untranslated, and a container that
            // is *accepted* while its interior vanishes publishes a complete
            // bound that omits whatever was inside.
            ast::Operator::FloorDiv
            | ast::Operator::Mod
            | ast::Operator::LShift
            | ast::Operator::RShift => {
                if approximation_of(binary, candidates, collections).is_some() {
                    vec![&binary.left]
                } else {
                    Vec::new()
                }
            }
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
fn postorder<'e>(root: &'e Expr, children: impl Fn(&'e Expr) -> Vec<&'e Expr>) -> Vec<&'e Expr> {
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

/// The position **just past** a node.
///
/// One caller: the implicit `__exit__` of a `with`, which runs after the last
/// statement of the block rather than at the `with` line. Two holes at one
/// position would also be indistinguishable to a consumer joining a hole to the
/// refusal that carries its specifics - that join is on position and construct -
/// so `__exit__` would be reported as `__enter__`.
fn origin_past<T: Ranged>(path: &Path, index: &LineIndex, node: &T) -> Origin {
    let (line, column) = index.position(node.end().to_usize());
    Origin::new(format!("{}:{line}:{column}", path.display()))
}
