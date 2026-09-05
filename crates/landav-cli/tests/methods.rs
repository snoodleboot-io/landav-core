//! `LAN-104`: **a method is a definition, and is lowered.**
//!
//! `lower_module` handed over only the definitions in `module.body`, so a
//! method was never a [`LoweredFunction`] - not refused, not holed, not in the
//! coverage denominator. Measured on the typed corpus, that was 35,413 methods
//! against 7,790 module-level functions, holding 69% of the `for` loops. Every
//! coverage figure recorded on the `LAN-89` to `LAN-103` ladder was over the
//! other 18%.
//!
//! What these tests pin: a method is lowered and named `Class.method`; the
//! decorators that change how a function is *called* do not keep its body out;
//! a nested definition still stays out, as a decision; and a class-body
//! binding does not shadow a builtin inside a method, because Python does not
//! resolve bare names that way.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use landav_engine::cost;
use landav_python::{LoweredFunction, lower_module};

fn lowered(source: &str) -> Vec<LoweredFunction> {
    lower_module(Path::new("methods.py"), source).expect("parses")
}

fn names(functions: &[LoweredFunction]) -> Vec<&str> {
    functions.iter().map(LoweredFunction::name).collect()
}

fn mentions(function: &LoweredFunction, name: &str) -> bool {
    cost(function.program())
        .bound()
        .is_some_and(|bound| bound.vars().iter().any(|var| var.symbol().as_str() == name))
}

/// **A method is lowered, named `Class.method`, in source order with the
/// module's functions.**
#[test]
fn a_method_is_lowered_under_its_class_name() {
    let functions = lowered(
        "\
def before(n: int) -> int:
    return n


class Widget:
    def count(self, items: list) -> int:
        total = 0
        for item in items:
            total += 1
        return total

    async def fetch(self, n: int) -> int:
        return n


def after(n: int) -> int:
    return n
",
    );
    assert_eq!(
        names(&functions),
        ["before", "Widget.count", "Widget.fetch", "after"]
    );
    assert_eq!(functions[1].class(), Some("Widget"));
    assert_eq!(functions[0].class(), None);
    assert_eq!(
        functions[1].location().line(),
        6,
        "a method reports at its own `def`, not at the class"
    );
}

/// **A method over a collection parameter derives a bound in its length,
/// exactly as the module-level function does.** The same translator, the same
/// answer; `self` is an unannotated parameter and claims nothing.
#[test]
fn a_method_over_a_collection_parameter_is_counted_by_its_length() {
    let functions = lowered(
        "\
class Widget:
    def count(self, items: list) -> int:
        total = 0
        for item in items:
            total += 1
        return total
",
    );
    assert!(
        mentions(&functions[0], "len(items)"),
        "the loop is over the caller's list. Got {:?}",
        cost(functions[0].program())
            .bound()
            .map(ToString::to_string)
    );
}

/// **A decorator does not keep a body out.** `@classmethod`, `@staticmethod`
/// and `@property` change how the function is called; the body costs what it
/// costs.
#[test]
fn decorated_methods_are_lowered() {
    let functions = lowered(
        "\
class Widget:
    @classmethod
    def build(cls, n: int) -> int:
        return n

    @staticmethod
    def helper(n: int) -> int:
        return n

    @property
    def size(self) -> int:
        return 1
",
    );
    assert_eq!(
        names(&functions),
        ["Widget.build", "Widget.helper", "Widget.size"]
    );
}

/// **A nested definition stays out, as a decision.** A `def` inside a
/// function, a class inside a function, and a class inside a class are all
/// reached only through a scope this walk does not enter.
#[test]
fn nested_definitions_are_still_not_lowered() {
    let functions = lowered(
        "\
def outer(n: int) -> int:
    def inner(m: int) -> int:
        return m

    class Local:
        def method(self) -> int:
            return 1

    return n


class Outer:
    class Inner:
        def method(self) -> int:
            return 1

    def own(self) -> int:
        return 1
",
    );
    assert_eq!(names(&functions), ["outer", "Outer.own"]);
}

/// **A class-body binding does not shadow a builtin inside a method.** Python
/// resolves a bare name in a method against the module and the builtins,
/// never against a class attribute, so `isinstance` here is the builtin, its
/// row applies, and the loop below the guard keeps its trip count. A
/// *module*-level binding still shadows, for a method as for a function.
///
/// The loop is over an integer on purpose: `isinstance`'s row keeps
/// `mutates_arguments = true` (an `__instancecheck__` is user code), so a
/// collection's length does not survive the guard, and that is the same for a
/// module-level function - it is not what this test is about.
#[test]
fn a_class_attribute_does_not_shadow_a_builtin_in_a_method() {
    let functions = lowered(
        "\
class Checker:
    isinstance = None

    def count(self, x, n: int) -> int:
        if isinstance(x, int):
            return 0
        total = 0
        for i in range(n):
            total += 1
        return total
",
    );
    assert!(
        mentions(&functions[0], "n"),
        "the class attribute named `isinstance` is not what the method calls. \
         Got {:?}",
        cost(functions[0].program())
            .bound()
            .map(ToString::to_string)
    );

    let shadowed = lowered(
        "\
def isinstance(x, t):
    return x


class Checker:
    def count(self, x, n: int) -> int:
        if isinstance(x, int):
            return 0
        total = 0
        for i in range(n):
            total += 1
        return total
",
    );
    assert!(
        !mentions(&shadowed[1], "n"),
        "a module-level `isinstance` is what the method calls, and it is \
         user code that may rebind any local. Got {:?}",
        cost(shadowed[1].program()).bound().map(ToString::to_string)
    );
}
