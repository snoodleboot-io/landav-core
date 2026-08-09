"""Reverse a stream of batches."""


def reverse_batches(batches):
    """Every ``insert(0, ...)`` shifts the whole list one place right."""
    ordered = []
    for batch in batches:
        ordered.insert(0, batch)  # LANDAV: LAV004 anchor=ordered.insert(0, batch)
    return ordered
