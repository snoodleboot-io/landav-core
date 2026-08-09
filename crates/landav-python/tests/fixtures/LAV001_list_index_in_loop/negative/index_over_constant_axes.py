"""Order the three spatial axes."""

AXES = ["x", "y", "z"]


def axis_order():
    """Three iterations, fixed at compile time."""
    order = []
    for axis in ("x", "y", "z"):
        order.append(AXES.index(axis))
    return order
