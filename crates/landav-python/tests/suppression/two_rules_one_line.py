"""One line, two rules, one comment naming both — and a foreign code beside them.

A waiver names each rule it waives. There is no spelling that means "all of
them", so silencing two rules costs two codes and leaves two records.
"""


def render(rows, items):
    out = ""
    for row in rows:
        # LANDAV-WAIVER: LAV001 status=applied count=1 reason=items has three entries in practice
        # LANDAV-WAIVER: LAV003 status=applied count=1 reason=items has three entries in practice
        out += str(items.index(row))  # noqa: LAV001, LAV003, E501 - items has three entries in practice
    return out
