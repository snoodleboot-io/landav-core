"""Read a length-prefixed record format straight off a file object."""


def read_records(handle):
    """A file object is its own iterator, so the nested loop is a continuation.

    The header says how many lines follow; the inner loop pulls exactly that
    many from the same handle. Neither loop rewinds, so the pair reads the file
    once and the function is linear in the number of lines.
    """
    records = []
    for header in handle:
        count = int(header.split()[-1])
        body = []
        for line in handle:
            body.append(line.rstrip())
            if len(body) == count:
                break
        records.append((header.rstrip(), body))
    return records
