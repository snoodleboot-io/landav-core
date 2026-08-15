"""A suppression that is data rather than a comment.

A line-oriented scanner honours both of the strings below and silences the
real finding at the bottom of the file. Deciding whether a `#` opens a comment
is a question only the parser can answer, which is why this lives behind the
frontend boundary.
"""

EXAMPLE = "out += piece  # noqa: LAV003"

HELP = """
Write the waiver next to the line it excuses:

    out += piece  # noqa: LAV003 - reason
"""


def render(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        out += str(row)
    return out
