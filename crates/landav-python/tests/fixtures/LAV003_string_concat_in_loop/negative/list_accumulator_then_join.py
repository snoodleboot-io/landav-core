"""Accumulate into a list, join at the end."""


def render_rows(rows):
    """``+=`` on a list is an amortised O(1) extend, not a copy."""
    chunks = []
    for row in rows:
        chunks += [f"{row.id}\t{row.label}\n"]
    return "".join(chunks)
