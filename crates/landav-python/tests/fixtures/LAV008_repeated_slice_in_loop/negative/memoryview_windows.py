"""Decode a fixed-width record stream without copying."""


def parse_records(payload):
    """``memoryview`` slices share storage, so the loop is linear."""
    view = memoryview(payload)
    offset = 0
    out = []
    while offset + 4 <= len(view):
        header = view[offset:offset + 4]
        out.append(int.from_bytes(header, "big"))
        offset += 4
    return out
