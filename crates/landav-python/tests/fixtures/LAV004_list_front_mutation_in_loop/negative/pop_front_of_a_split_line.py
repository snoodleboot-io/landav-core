"""Read a directive file: ``NAME arg arg arg`` per line."""


def parse_directives(lines):
    """``parts`` is this line's fields, not the whole file.

    ``parts.pop(0)`` shifts the fields of one line, so the loop total is the
    number of fields in the input — linear. A deque here would cost an extra
    allocation per line to save nothing.
    """
    directives = []
    for line in lines:
        parts = line.split()
        if not parts:
            continue
        name = parts.pop(0)
        directives.append((name, parts))
    return directives
