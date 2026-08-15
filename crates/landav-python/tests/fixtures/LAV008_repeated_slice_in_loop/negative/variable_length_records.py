"""Decode a container whose directory gives each record's offset and length."""


def _decode(payload):
    return payload.decode("utf-8", "replace")


def parse_records(blob, directory):
    """Every byte of ``blob`` is copied at most once across the whole loop.

    The record length is per record, so the window is not a compile-time
    constant, but the windows tile the blob rather than nesting inside one
    another. Summed over the directory this is O(len(blob)).
    """
    records = []
    for header in directory:
        length = header.length
        payload = blob[header.offset:header.offset + length]
        records.append(_decode(payload))
    return records
