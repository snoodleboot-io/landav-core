//! The pattern-rule registry: every rule this frontend can emit.

use crate::{rule::Rule, rule_code::RuleCode};

/// Every rule this frontend can emit, in ascending [`RuleCode`] order.
///
/// The ordering is part of the contract: `landav explain --list` and the CI
/// baseline both iterate it, and an order that depends on declaration order is
/// an order that changes when somebody inserts a rule in the middle.
#[must_use]
pub fn registry() -> &'static [Rule] {
    RULES
}

/// Looks a rule up by its code. `None` for a code this build does not know.
#[must_use]
pub fn rule_for_code(code: &str) -> Option<&'static Rule> {
    registry().iter().find(|rule| rule.code().as_str() == code)
}

/// Looks a rule up by an already-validated [`RuleCode`].
#[must_use]
pub fn rule(code: RuleCode) -> Option<&'static Rule> {
    rule_for_code(code.as_str())
}

// TODO(LAN-65): the implementation lane populates this table. It is empty on
// purpose — the fixture corpus under `tests/fixtures/` and the assertions in
// `tests/rule_registry.rs` are the specification of what belongs in it, and
// they were written before any rule existed.
static RULES: &[Rule] = &[];
