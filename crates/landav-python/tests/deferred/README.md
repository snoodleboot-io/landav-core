# `tests/deferred/` — evidence the harness deliberately does not walk

Nothing under this directory is executed, analysed or asserted. The corpus
harness in `tests/common/mod.rs` roots itself at `tests/fixtures/`, and
`tests/fixture_corpus.rs::deferred_tree_is_not_walked` asserts that no file
under here reaches it. Moving a directory back into `tests/fixtures/` is the
only thing that makes it live again.

It exists because two kinds of work are worth more than they cost to keep, and
both are lost if the only record is a deleted directory or a commit message:

| Directory | What it is |
|---|---|
| `LAV010_exception_as_control_flow_in_loop/` | A withdrawn rule, with the fixtures that killed it |
| `known_gaps/` | Defects the current rules cannot see, recorded so nobody assumes they are covered |

**These files are not fixtures.** They carry no `# LANDAV:` markers that mean
anything to the harness, they assert nothing, and a rule change cannot break
them. They are kept `py_compile`-clean so that a future reader can run them
through a parser without first repairing them.

Each subdirectory has its own `README.md` stating what it is and what would
have to change for it to move.
