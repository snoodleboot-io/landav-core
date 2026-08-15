"""A reason carrying the punctuation a real justification contains.

The reason is free text and is stored verbatim from the first non-code token
onward, so a colon inside it does not truncate it and the leading dash is not
part of it.
"""


def summarise(rows, allowed):
    known = list(allowed)
    out = []
    for row in rows:
        # LANDAV-WAIVER: LAV002 status=applied count=1 reason=allowed: never more than four entries
        if row in known:  # noqa: LAV002 - allowed: never more than four entries
            out.append(str(row))
    return "".join(out)
