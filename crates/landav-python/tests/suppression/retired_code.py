"""A waiver naming LAV010, which was issued and then withdrawn.

The author spelled the code correctly; the rule beneath it no longer exists.
Reporting that as a typo would send them looking for the wrong mistake, which
is the whole reason a retired number is burned rather than recycled.
"""


def render(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        # LANDAV-WAIVER: LAV010 status=retired count=0 reason=the guarded lookup is deliberate
        out += str(row)  # noqa: LAV010 - the guarded lookup is deliberate
    return out
