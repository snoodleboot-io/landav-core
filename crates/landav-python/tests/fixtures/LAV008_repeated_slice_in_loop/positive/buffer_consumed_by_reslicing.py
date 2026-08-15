"""Decode a fixed-width record stream.

``payload[:4]`` is a four-byte copy and must not be reported; ``payload[4:]``
copies everything that is left and must be.
"""


def decode(header):
    """Placeholder decoder."""
    return int.from_bytes(header, "big")


def parse_records(payload):
    """Each tail slice copies the rest of the buffer."""
    out = []
    while payload:
        header = payload[:4]
        payload = payload[4:]  # LANDAV: LAV008 anchor=payload[4:]
        out.append(decode(header))
    return out
