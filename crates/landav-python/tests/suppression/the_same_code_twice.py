"""The same code waived on two different lines.

Each line is its own waiver with its own reason and its own record, so a
reviewer removing one does not silently remove the other. A scanner that
folded them into one would report half of what is actually in the file.
"""


def render(rows):
    out = ""
    for row in rows:
        # LANDAV-WAIVER: LAV003 status=applied count=1 reason=the header is three lines
        out += str(row)  # noqa: LAV003 - the header is three lines
    return out


def render_footer(rows):
    out = ""
    for row in rows:
        # LANDAV-WAIVER: LAV003 status=applied count=1 reason=the footer is one line
        out += str(row)  # noqa: LAV003 - the footer is one line
    return out
