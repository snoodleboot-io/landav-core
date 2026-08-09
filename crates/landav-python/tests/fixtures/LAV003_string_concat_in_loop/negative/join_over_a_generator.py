"""Render rows, the linear way."""


def render_rows(rows):
    """One allocation sized once, linear total work."""
    return "".join(f"{row.id}\t{row.label}\n" for row in rows)
