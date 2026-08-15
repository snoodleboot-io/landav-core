"""Strip the eight-byte header off each row's buffer."""


def payloads(rows):
    """Each row slices its own buffer; the total copy is linear in the input."""
    out = []
    for row in rows:
        out.append(row.raw[8:])
    return out
