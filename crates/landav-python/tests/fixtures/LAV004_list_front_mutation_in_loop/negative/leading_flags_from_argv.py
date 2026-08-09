"""A hand-rolled leading-flag parser, the shape ``getopt`` callers write."""


def split_leading_flags(argv):
    """``argv`` is bounded by the command line, and order is the whole point.

    The loop stops at the first non-flag, so it runs at most as many times as
    there are leading flags — a handful. Consuming from the front is what makes
    the remainder a plain list the caller can pass on unchanged.
    """
    flags = []
    remaining = list(argv)
    while remaining and remaining[0].startswith("-") and remaining[0] != "--":
        flags.append(remaining.pop(0))
    return flags, remaining
