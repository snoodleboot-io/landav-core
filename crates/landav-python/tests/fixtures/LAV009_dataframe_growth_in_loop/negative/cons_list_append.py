"""A cons list, where appending is a single allocation."""


class Chain:
    """An immutable stack; ``append`` links a new head in constant time."""

    __slots__ = ("item", "rest")

    def __init__(self, item, rest=None):
        self.item = item
        self.rest = rest

    def append(self, item):
        """O(1): links ``item`` in front of ``self`` and returns the new head."""
        return Chain(item, self)


def chain_of(items):
    """Rebinding the head is the only way to use a persistent structure."""
    chain = Chain(None)
    for item in items:
        chain = chain.append(item)
    return chain
