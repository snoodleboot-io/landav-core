"""A waiver one line above the finding.

It covers the line it is written on and no other. Quietly extending it to a
neighbour would waive a line its author never looked at, which is the blanket
problem wearing a smaller hat. The finding stands and the waiver is reported
as unused, so the author can see that they missed.
"""


def render(rows):
    out = ""
    # LANDAV-WAIVER: LAV003 status=unused count=0
    for row in rows:  # noqa: LAV003
        # LANDAV-FINDING: LAV003
        out += str(row)
    return out
