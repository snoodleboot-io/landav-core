"""Number a list of names for a report."""


def numbered_names(names):
    """Return "3: alice" style lines for every name."""
    lines = []
    for name in names:
        position = names.index(name)  # LANDAV: LAV001 anchor=names.index(name)
        lines.append(f"{position}: {name}")
    return lines
