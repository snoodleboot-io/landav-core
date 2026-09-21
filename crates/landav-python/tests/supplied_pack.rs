//! `LAN-13`: a pack handed to the lowering is the table the lowering uses.
//!
//! # What this is guarding
//!
//! [`landav_python::lower_module_with`] grew a pack parameter, and plumbing is
//! the part of a change that fails quietly: a seam that accepts a pack and then
//! resolves against the builtin anyway type-checks, passes every existing test,
//! and makes `--signatures` a flag that appears to work. The only assertion
//! that catches it is one where the two tables give *different* answers to the
//! same source.
//!
//! So `frobnicate` is a name the builtin has never heard of. With no pack the
//! call is an undeclared node - a hole. With a pack that declares it, the node
//! carries the effect the row declared and stops being one. A lowering wired to
//! the wrong table cannot produce both.
//!
//! Note what is *not* asserted: that the node disappears. It does not. A
//! resolved call is still a node in the arena, and the difference a pack makes
//! is [`landav_its::UnsupportedNode::declared`] - which is the distinction
//! between a bound with a hole in it and a bound without one.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use landav_fdk::{PackOrigin, SignaturePack};
use landav_python::AnnotationTrust;

/// A module calling a name no builtin row claims.
const SOURCE: &str = "\
def run(n: int) -> int:
    frobnicate(n)
    return n
";

/// A pack declaring `frobnicate` resolvable, as if somebody measured it.
fn supplied() -> SignaturePack {
    let text = "\
[[signature]]
callee = \"frobnicate\"
cost = \"constant\"
rebinds_locals = false
mutates_arguments = true
why = \"a table lookup, measured on our deployment\"
";
    SignaturePack::parse(&PackOrigin::File(PathBuf::from("team.toml")), text)
        .expect("the supplied pack parses")
}

/// Whether the `frobnicate` node carries a declaration when lowered with `pack`.
///
/// `None` means the node is a hole: the lowering found no row it could use.
/// `Some` means a row declared the call's effect and the bound closes over it.
fn frobnicate_is_declared(pack: Option<&SignaturePack>) -> bool {
    let functions = landav_python::lower_module_with(
        Path::new("m.py"),
        SOURCE,
        AnnotationTrust::default(),
        pack,
    )
    .expect("the source is inside the fragment");
    let program = functions
        .into_iter()
        .next()
        .expect("one function")
        .into_program();
    let mut nodes = program.unsupported_nodes().filter(|node| {
        node.detail()
            .is_some_and(|name| name.as_str() == "frobnicate")
    });
    let node = nodes
        .next()
        .expect("the call to `frobnicate` is a node either way");
    node.declared().is_some()
}

#[test]
fn without_a_pack_an_unknown_callee_is_an_undeclared_hole() {
    assert!(
        !frobnicate_is_declared(None),
        "the builtin has no row for `frobnicate`, so the call must stay a \
         hole - if this fails the test below proves nothing, because both \
         tables would be giving the same answer"
    );
}

#[test]
fn a_supplied_pack_is_the_table_the_lowering_resolves_against() {
    let pack = supplied();
    assert!(
        frobnicate_is_declared(Some(&pack)),
        "the pack was accepted at the seam and then ignored: the lowering \
         resolved against the builtin, which is what makes a supplied pack a \
         flag that only appears to work"
    );
}

/// The supplied pack replaces the table; it does not empty it.
///
/// The other way plumbing fails: handing the lowering *only* the supplied pack
/// would declare `frobnicate` and lose `isinstance`, turning a one-row pack
/// into a coverage regression across every file that uses a builtin row.
#[test]
fn a_supplied_pack_does_not_cost_the_builtin_rows() {
    let mut pack = SignaturePack::builtin().expect("the builtin pack parses");
    pack.overlay(supplied());
    assert!(
        pack.row("isinstance").is_some(),
        "overlaying a one-row pack dropped the builtin rows"
    );
    assert!(frobnicate_is_declared(Some(&pack)));
}
