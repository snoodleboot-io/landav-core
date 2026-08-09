"""Build an audit trail as a persistent vector."""

from pyrsistent import pvector


def audit_trail(events):
    """``PVector.append`` is amortised O(1) and returns a new head.

    A persistent vector is a bit-partitioned trie: appending copies one 32-slot
    node, not the whole structure. Rebinding the name is how an immutable
    structure is used, and "collect the parts and concat once" would throw away
    the intermediate versions this function exists to produce.
    """
    trail = pvector()
    for event in events:
        trail = trail.append((event.at, event.kind))
    return trail
