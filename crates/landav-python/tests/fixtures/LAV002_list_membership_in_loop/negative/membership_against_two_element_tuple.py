"""Select healthy rows."""


def healthy_rows(rows):
    """Two comparisons; a set here would be slower, not faster."""
    return [row for row in rows if row.status in ("ok", "warn")]
