"""Render rows into one report string."""


def format_row(row):
    """One rendered line."""
    return f"{row.id}\t{row.label}\n"


def render_rows(rows):
    """``out`` is copied in full on every ``+=``."""
    out = ""
    for row in rows:
        out += format_row(row)  # LANDAV: LAV003 anchor=out += format_row(row)
    return out
