//! `LAN-130`: **a function is reported at its `def`, not at its decorators.**
//!
//! The parser swap found this by measurement rather than by reasoning, which
//! is why it has a test of its own. `ruff_python_parser` gives a decorated
//! `def` a range that **starts at the first `@`**, where the previous parser
//! started at the keyword; taking the range's own start moved 10,802 of the
//! momentum corpus's 43,203 functions earlier by exactly their decorator
//! count, and nothing else about them changed. A silent shift, in other words,
//! of the kind only a before-and-after over a real corpus surfaces.
//!
//! It matters because the position is a published value, not a cosmetic one:
//!
//! * `Function.line` and `Function.column` are in the machine schema
//!   (`LAN-84`), so a consumer that stored them would see every decorated
//!   function move without a schema change to explain it;
//! * every other Python tool - a traceback, a linter, a coverage report -
//!   names the `def` line, and a tool that disagrees sends a reader to the
//!   wrong line;
//! * `Origin` strings are joined to holes on position, so two functions whose
//!   positions collapse onto one decorator line would be indistinguishable.
//!
//! What is asserted here is the rule, in the shapes that can break it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io;

use common::Project;
use serde_json::Value;

/// Every function in `source`, as `name -> (line, column)`.
fn positions(source: &str) -> io::Result<Vec<(String, u64, u64)>> {
    let project = Project::new()?;
    let file = project.write("w.py", source)?;
    let run = project.check(&file, &["--json"])?;
    let doc: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|why| panic!("stdout was not JSON ({why}): {}", run.describe()));
    Ok(doc["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().to_owned(),
                f["line"].as_u64().unwrap(),
                f["column"].as_u64().unwrap(),
            )
        })
        .collect())
}

fn at(found: &[(String, u64, u64)], name: &str) -> (u64, u64) {
    found
        .iter()
        .find(|(it, _, _)| it == name)
        .map(|(_, line, column)| (*line, *column))
        .unwrap_or_else(|| panic!("no function {name} in {found:?}"))
}

/// **One decorator, several, and none.**
#[test]
fn a_decorated_function_is_reported_at_its_def() -> io::Result<()> {
    // Line numbers are counted from 1 and written beside each `def`.
    let found = positions(
        "\
@one
def single(n: int) -> int:
    return 1


@one
@two
@three
def several(n: int) -> int:
    return 1


def none(n: int) -> int:
    return 1
",
    )?;
    assert_eq!(at(&found, "single"), (2, 1), "{found:?}");
    assert_eq!(at(&found, "several"), (9, 1), "{found:?}");
    assert_eq!(at(&found, "none"), (13, 1), "{found:?}");
    Ok(())
}

/// **A comment or a blank line between the decorator and the `def`.**
///
/// The scan has to step over both, or the position lands on the comment.
#[test]
fn a_comment_between_the_decorator_and_the_def_is_skipped() -> io::Result<()> {
    let found = positions(
        "\
@one
# why this is decorated

# and a second note
def spaced(n: int) -> int:
    return 1
",
    )?;
    assert_eq!(at(&found, "spaced"), (5, 1), "{found:?}");
    Ok(())
}

/// **`async def` keeps pointing at `async`, decorated or not.**
///
/// The column is the assertion: `async` is five characters before `def`, and
/// the position has always named the keyword the reader sees first.
#[test]
fn an_async_def_is_reported_at_the_async_keyword() -> io::Result<()> {
    let found = positions(
        "\
@one
async def decorated(n: int) -> int:
    return 1


async def plain(n: int) -> int:
    return 1
",
    )?;
    assert_eq!(at(&found, "decorated"), (2, 1), "{found:?}");
    assert_eq!(at(&found, "plain"), (6, 1), "{found:?}");
    Ok(())
}

/// **A decorated method inside a decorated class.**
///
/// Both levels carry decorators, and the method's column is its indentation -
/// the `def`'s column, not the `@`'s, though here they agree - while its line
/// must be the `def`.
#[test]
fn a_decorated_method_in_a_decorated_class_is_reported_at_its_def() -> io::Result<()> {
    let found = positions(
        "\
@register
class Holder:
    @property
    def size(self) -> int:
        return 1

    @staticmethod
    @cached
    def build(n: int) -> int:
        return 1

    def undecorated(self, n: int) -> int:
        return 1
",
    )?;
    assert_eq!(at(&found, "Holder.size"), (4, 5), "{found:?}");
    assert_eq!(at(&found, "Holder.build"), (9, 5), "{found:?}");
    assert_eq!(at(&found, "Holder.undecorated"), (12, 5), "{found:?}");
    Ok(())
}

/// **A decorator carrying arguments, a call chain, and a line continuation.**
///
/// The scan starts at the end of the *last* decorator, so a decorator whose
/// own expression spans lines must not leave the position inside it.
#[test]
fn a_multi_line_decorator_expression_does_not_hold_the_position() -> io::Result<()> {
    let found = positions(
        "\
@registry.register(
    name=\"thing\",
    kind=\"other\",
)
def configured(n: int) -> int:
    return 1


@one \\

def continued(n: int) -> int:
    return 1
",
    )?;
    assert_eq!(at(&found, "configured"), (5, 1), "{found:?}");
    assert_eq!(at(&found, "continued"), (11, 1), "{found:?}");
    Ok(())
}
