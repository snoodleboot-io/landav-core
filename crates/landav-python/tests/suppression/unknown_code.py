"""Waivers aimed at landav that name no rule it has ever carried.

The dangerous case: without a report, the author believes the finding below is
waived, and it is not.
"""


def render(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        # LANDAV-WAIVER: LAV03 status=unknown count=0
        out += str(row)  # noqa: LAV03
    return out


def render_lowercase(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        # LANDAV-WAIVER: lav003 status=unknown count=0
        out += str(row)  # noqa: lav003
    return out
