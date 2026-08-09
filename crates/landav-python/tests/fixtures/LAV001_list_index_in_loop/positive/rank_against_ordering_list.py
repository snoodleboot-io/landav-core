"""Score items against a canonical ordering."""


def rank_by_ordering(items, ordering):
    """Map each item to its position in ``ordering``.

    ``ordering`` is a list, so every lookup walks it from the front.
    """
    ranks = {}
    for item in items:
        ranks[item] = ordering.index(item)  # LANDAV: LAV001 anchor=ordering.index(item)
    return ranks
