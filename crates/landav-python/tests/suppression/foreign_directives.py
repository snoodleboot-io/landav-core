"""Directives belonging to other tools. Landav claims none of them.

A repository that already runs ruff or flake8 is full of these. Honouring them
would let another tool's suppression silence landav; complaining about them
would make landav the noisy one. It does neither.
"""


def render_other_tools_code(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        out += str(row)  # noqa: E501
    return out


def render_blanket(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        out += str(row)  # noqa
    return out


def render_type_directive(rows):
    out = ""
    for row in rows:
        # LANDAV-FINDING: LAV003
        out += str(row)  # type: ignore
    return out
