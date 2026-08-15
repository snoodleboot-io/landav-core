"""A quadratic accumulation the author has judged acceptable, waived in place.

The canonical case: one line, one rule, a reason next to the code it excuses.
"""


def render(rows):
    out = ""
    for row in rows:
        # LANDAV-WAIVER: LAV003 status=applied count=1 reason=at most a dozen rows; see LAN-70
        out += str(row)  # noqa: LAV003 - at most a dozen rows; see LAN-70
    return out
