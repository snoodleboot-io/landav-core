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
    // A bare `->` is cost one to KoAT, so the common case needs no annotation
    // and step-counting systems render exactly as they did before costs were
    // representable. Anything else is spelled out, including zero - `-{0}>` is
    // read as free, and verified so against the solver rather than assumed.
    if transition.cost().is_step() {
        rule.push_str(") -> ");
    } else {
        rule.push_str(") -{");
        rule.push_str(&render_polynomial(transition.cost().polynomial()));
        rule.push_str("}> ");
    }
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

#[cfg(test)]
mod tests {
    use landav_bound::{Origin, Symbol};

    use super::render_rule;
    use crate::{
        cost::Cost, guard::Guard, its::Its, its_var::ItsVar, location_id::LocationId,
        polynomial::Polynomial, transition::Transition, update::Update,
    };

    /// The cost annotation is the one piece of this format whose syntax was
    /// established by probing KoAT rather than by reading a grammar. `-{c}>` was
    /// confirmed to move the solver's answer in the expected direction - a rule
    /// charged two units per iteration reports `2*Arg_0+3` where the unweighted
    /// form reports `Arg_0+1` - and `-{0}>` was confirmed to be read as free
    /// rather than ignored.
    ///
    /// So these assert an empirical fact about the solver, not a restatement of
    /// the code above. If a future KoAT changes the syntax, this is where it
    /// surfaces, and the fix is to re-probe rather than to guess again.
    fn rule_for(cost: Cost) -> String {
        // Built directly rather than lowered: the lowering emits only unit
        // costs, so a lowered system cannot exercise the annotated branch at
        // all. That gap closes when the trip-count work starts charging real
        // costs; until then this is the only way to test the renderer.
        let its = Its {
            name: Symbol::from("probe"),
            origin: Origin::new("probe"),
            vars: vec![ItsVar::new(Symbol::from("n"))],
            params: Vec::new(),
            start: LocationId(0),
            exit: LocationId(1),
            locations: Vec::new(),
            transitions: Vec::new(),
        };
        let transition = Transition::new(
            LocationId(0),
            LocationId(1),
            Guard::always(),
            Update::identity(),
            cost,
            Origin::new("probe"),
        );
        render_rule(&its, &transition)
    }

    #[test]
    fn a_unit_cost_renders_as_a_bare_arrow() {
        let rule = rule_for(Cost::step());
        assert!(rule.contains(") -> "), "got {rule}");
        assert!(!rule.contains("-{"), "unit cost should stay implicit: {rule}");
    }

    #[test]
    fn a_charged_transition_shows_its_cost() {
        let rule = rule_for(Cost::constant(3));
        assert!(rule.contains(") -{3}> "), "got {rule}");
    }

    /// Zero is the case a "annotate when non-default" rule gets wrong if it
    /// tests truthiness instead of equality with one. A dropped `-{0}>` silently
    /// becomes a step, which inflates every bound routed through that edge.
    #[test]
    fn a_free_transition_is_annotated_rather_than_omitted() {
        let rule = rule_for(Cost::free());
        assert!(rule.contains(") -{0}> "), "zero must be explicit: {rule}");
    }

    #[test]
    fn a_stateful_cost_renders_as_its_polynomial() {
        let rule = rule_for(Cost::stateful(Polynomial::var(ItsVar::new(Symbol::from(
            "n",
        )))));
        assert!(rule.contains(") -{vn}> "), "got {rule}");
    }
}
