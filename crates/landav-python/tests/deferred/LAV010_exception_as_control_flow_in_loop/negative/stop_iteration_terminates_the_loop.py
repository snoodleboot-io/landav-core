"""Pull items from an iterator until it is exhausted."""


def take_until_exhausted(source, process):
    """``StopIteration`` is raised exactly once for the whole loop."""
    seen = 0
    while True:
        try:
            item = next(source)
        except StopIteration:
            break
        process(item)
        seen += 1
    return seen
