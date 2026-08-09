"""Render each record's tags in a stable order."""


def render_tags(records):
    """Each record sorts its own tags; nothing accumulates across iterations."""
    lines = []
    for record in records:
        tags = sorted(record.tags)
        lines.append(",".join(tags))
    return lines
