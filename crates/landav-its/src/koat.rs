//! Rendering an [`Its`] as KoAT's integer transition system format.
//!
//! # The format
//!
//! ```text
//! (GOAL COMPLEXITY)
//! (STARTTERM (FUNCTIONSYMBOLS l0))
//! (VAR vi vn)
//! (RULES
//!   l0(vi,vn) -> l1(0,vn)
//!   l1(vi,vn) -> l1(vi + 1,vn) :|: vn - vi > 0
//!   l1(vi,vn) -> l2(vi,vn) :|: vi - vn >= 0
//! )
//! ```
//!
//! Locations take the whole variable tuple positionally and in the same order
//! everywhere, so a rule's right-hand side is a full description of the
//! post-state: variables an [`crate::Update`] does not mention are written out
//! as themselves rather than omitted.
//!
//! # Two deliberate conservatisms
//!
//! **Powers are expanded.** `x^3` is written `x*x*x`. KoAT's own parser
//! accepts `^` with a literal exponent, but ITS-format dialects disagree about
//! it and the expansion is free - [`crate::MAX_DEGREE`] bounds how long it can
//! get. There is no reason to spend compatibility on notation.
//!
//! **`!=` never appears.** It is eliminated into a disjunction during the
//! normal-form step, so every guard emitted here is a plain conjunction of
//! `>=`, `>` and `=`. See [`crate::Relation`].
//!
//! # Identifier escaping
//!
//! A frontend-supplied variable name is arbitrary text - it may contain
//! spaces, punctuation, or non-ASCII - and KoAT identifiers are not. Names are
//! therefore escaped by [`mangle`], which is **injective**: two distinct
//! source variables can never become one KoAT variable. That is a soundness
//! property rather than a cosmetic one. Merging two variables into one would
//! silently equate them, which changes what every guard mentioning either one
//! means, and the change is not in the safe direction.

use crate::{
    constraint::Constraint, guard::Guard, its::Its, its_var::ItsVar, polynomial::Polynomial,
    transition::Transition,
};

/// The escape character used by [`mangle`].
///
/// `Q` because it is rare in identifiers, so most names survive unchanged and
/// stay readable in the emitted text.
const ESCAPE: char = 'Q';

/// The prefix every mangled variable takes.
///
/// Guarantees the result starts with a letter, and - because locations are
/// rendered `l0`, `l1`, ... - guarantees no variable can ever collide with a
/// location name.
const VAR_PREFIX: char = 'v';

/// The system as KoAT input.
#[must_use]
pub fn render(its: &Its) -> String {
    let mut out = String::new();
    out.push_str("(GOAL COMPLEXITY)\n");
    out.push_str(&format!("(STARTTERM (FUNCTIONSYMBOLS {}))\n", its.start()));

    out.push_str("(VAR");
    for var in its.vars() {
        out.push(' ');
        out.push_str(&mangle(var));
    }
    out.push_str(")\n");

    out.push_str("(RULES\n");
    for transition in its.transitions() {
        out.push_str("  ");
        out.push_str(&render_rule(its, transition));
        out.push('\n');
    }
    out.push_str(")\n");
    out
}

/// One rule.
fn render_rule(its: &Its, transition: &Transition) -> String {
    let mut rule = String::new();
    rule.push_str(&transition.source().to_string());
    rule.push('(');
    for (index, var) in its.vars().iter().enumerate() {
        if index > 0 {
            rule.push(',');
        }
        rule.push_str(&mangle(var));
    }
    rule.push_str(") -> ");
    rule.push_str(&transition.target().to_string());
    rule.push('(');
    for (index, var) in its.vars().iter().enumerate() {
        if index > 0 {
            rule.push(',');
        }
        match transition.update().get(var) {
            // An unmentioned variable is unchanged, and the format has no way
            // to say that except by writing it out.
            None => rule.push_str(&mangle(var)),
            Some(value) => rule.push_str(&render_polynomial(value)),
        }
    }
    rule.push(')');

    if !transition.guard().is_always() {
        rule.push_str(" :|: ");
        rule.push_str(&render_guard(transition.guard()));
    }
    rule
}

/// A guard as a conjunction.
fn render_guard(guard: &Guard) -> String {
    guard
        .constraints()
        .iter()
        .map(render_constraint)
        .collect::<Vec<String>>()
        .join(" && ")
}

/// One constraint, as `p R 0`.
fn render_constraint(constraint: &Constraint) -> String {
    format!(
        "{} {} 0",
        render_polynomial(constraint.polynomial()),
        constraint.relation()
    )
}

/// A polynomial, with powers expanded to repeated multiplication.
#[must_use]
pub fn render_polynomial(polynomial: &Polynomial) -> String {
    if polynomial.is_zero() {
        return "0".to_owned();
    }
    let mut out = String::new();
    for (index, term) in polynomial.monomials().iter().enumerate() {
        // A zero coefficient never reaches here: `Polynomial::from_monomials`
        // filters them and cancels like terms, so every surviving monomial has
        // a non-zero coefficient. That makes `< 0` and `<= 0` agree on every
        // input this function can receive -- mutation testing reports the
        // swap as a surviving mutant, and it is a genuinely equivalent one.
        // Killing it would mean making a zero coefficient representable, which
        // would be a worse crate for a better number.
        let coefficient = term.coefficient();
        if index > 0 {
            out.push_str(if coefficient < 0 { " - " } else { " + " });
        } else if coefficient < 0 {
            out.push('-');
        }
        // `unsigned_abs`, not `abs`: `i64::MIN.abs()` is not an `i64`.
        let magnitude = coefficient.unsigned_abs();
        let explicit = magnitude != 1 || term.is_constant();
        if explicit {
            out.push_str(&magnitude.to_string());
        }
        let mut first_factor = !explicit;
        for (var, exponent) in term.powers() {
            for _ in 0..*exponent {
                if first_factor {
                    first_factor = false;
                } else {
                    out.push('*');
                }
                out.push_str(&mangle(var));
            }
        }
    }
    out
}

/// A variable name as a KoAT identifier.
///
/// # Injectivity
///
/// The encoding is `v` followed by a per-character mapping:
///
/// * an ASCII letter, digit or underscore other than `Q` maps to
///   itself;
/// * `Q` maps to `QQ`;
/// * anything else maps to `Q`, the Unicode scalar value in lowercase hex,
///   then `Q`.
///
/// Decoding is unambiguous - after a `Q`, a second `Q` is a literal and
/// anything else is hex up to the closing `Q` - so distinct names produce
/// distinct output. The property suite asserts injectivity over generated
/// names rather than taking this paragraph's word for it.
#[must_use]
pub fn mangle(var: &ItsVar) -> String {
    let mut out = String::with_capacity(var.as_str().len() + 1);
    out.push(VAR_PREFIX);
    for character in var.as_str().chars() {
        if character == ESCAPE {
            out.push(ESCAPE);
            out.push(ESCAPE);
        } else if character.is_ascii_alphanumeric() || character == '_' {
            out.push(character);
        } else {
            out.push(ESCAPE);
            out.push_str(&format!("{:x}", u32::from(character)));
            out.push(ESCAPE);
        }
    }
    out
}
