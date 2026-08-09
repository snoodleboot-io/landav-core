"""Drain a work queue held in a list."""


def handle(item):
    """Placeholder for the real work."""
    return item


def drain(queue):
    """``pop(0)`` shifts every remaining element down one slot."""
    processed = []
    while queue:
        item = queue.pop(0)  # LANDAV: LAV004 anchor=queue.pop(0)
        processed.append(handle(item))
    return processed
