"""Two normalisation passes over a node list."""


def normalise(nodes):
    """Two linear passes, not a quadratic one."""
    for _ in range(2):
        for node in nodes:
            node.rank = node.rank * 2
    return nodes
