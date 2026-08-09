"""Flatten records to (key, field, value) triples."""

FIELDS = ("name", "email", "id")


def flatten(records):
    """Three inner iterations, fixed at compile time."""
    rows = []
    for record in records:
        for field in FIELDS:
            rows.append((record.key, field, getattr(record, field)))
    return rows
