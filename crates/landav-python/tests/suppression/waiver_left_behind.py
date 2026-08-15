"""A waiver still sitting on a line whose defect was fixed years ago.

This is the case criterion 3 exists for. Nothing is suppressed, nothing fails,
and the waiver is reported so that somebody can delete it.
"""


def render(rows):
    pieces = []
    for row in rows:
        # LANDAV-WAIVER: LAV003 status=unused count=0
        pieces.append(str(row))  # noqa: LAV003
    return "".join(pieces)
