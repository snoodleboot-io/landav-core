"""Prepend a header row to a rendered table."""

HEADER = "id\tlabel"


def render_table(records):
    """One shift of the whole list, outside any loop."""
    rows = [f"{record.id}\t{record.label}" for record in records]
    rows.insert(0, HEADER)
    return rows
