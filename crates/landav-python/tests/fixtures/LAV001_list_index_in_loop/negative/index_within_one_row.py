"""Locate a column inside each row of a ragged CSV export."""


def value_after(rows, marker):
    """The scan is over one row, so the loop total is the size of the input.

    Each row carries its own header cell, so there is no single ordering to
    build a position map from; ``cells.index`` walks at most one row.
    """
    values = []
    for row in rows:
        cells = row.split(",")
        if marker not in cells:
            continue
        position = cells.index(marker)
        values.append(cells[position + 1])
    return values
