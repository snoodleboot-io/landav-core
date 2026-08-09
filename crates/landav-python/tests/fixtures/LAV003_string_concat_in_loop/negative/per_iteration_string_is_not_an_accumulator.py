"""Build one string per row."""


def render_rows(rows):
    """``line`` is reset every iteration, so the total work is linear."""
    out = []
    for row in rows:
        line = ""
        line += row.label
        line += "\n"
        out.append(line)
    return out
