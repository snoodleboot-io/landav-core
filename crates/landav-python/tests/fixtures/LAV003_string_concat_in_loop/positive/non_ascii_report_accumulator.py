"""Render a French-language summary.

The multi-byte literals below sit above the finding on purpose: a rule that
derives a column from a whole-file byte offset without resetting at each
newline gets this file wrong and every ASCII file right.
"""

HEADING = "Résumé des évènements — total"
SEPARATOR = " · "


def render_summary(rows):
    """Accumulate the report one row at a time."""
    text = HEADING + "\n"
    for row in rows:
        text += SEPARATOR + row.label  # LANDAV: LAV003 anchor=text += SEPARATOR + row.label
    return text
