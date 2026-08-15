"""Produce an ascending and a descending view."""


def both_orders(rows):
    """Two sorts, whatever the row count."""
    views = []
    for descending in (False, True):
        rows.sort(key=lambda row: row.score, reverse=descending)
        views.append(list(rows))
    return views
