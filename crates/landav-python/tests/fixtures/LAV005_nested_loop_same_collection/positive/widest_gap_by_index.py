"""Widest distance between any two points."""


def distance(left, right):
    """Placeholder metric."""
    return abs(left - right)


def widest_gap(points):
    """Both ranges are driven by the same length."""
    widest = 0.0
    for i in range(len(points)):
        for j in range(len(points)):  # LANDAV: LAV005 anchor=for j in range(len(points)):
            widest = max(widest, distance(points[i], points[j]))
    return widest
