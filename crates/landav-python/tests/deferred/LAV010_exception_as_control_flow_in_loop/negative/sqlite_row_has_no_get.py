"""Read an optional column out of ``sqlite3.Row`` objects."""


def notes(cursor):
    """``sqlite3.Row`` has no ``.get``; the handler is the only way to ask.

    ``Row.__getitem__`` raises ``IndexError`` for a column the query did not
    select, and the class exposes no total lookup. ``dict(row).get("note")``
    builds a dict per row, which is strictly more work than the handler.
    """
    collected = []
    for row in cursor:
        try:
            note = row["note"]
        except IndexError:
            note = ""
        collected.append(note)
    return collected
