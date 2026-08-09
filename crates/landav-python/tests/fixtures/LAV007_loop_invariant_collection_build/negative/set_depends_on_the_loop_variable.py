"""Report which of each event's tags are wanted."""


def tag_overlaps(events, wanted):
    """``set(event.tags)`` differs every iteration; there is nothing to hoist."""
    overlaps = []
    for event in events:
        tags = set(event.tags)
        overlaps.append((event.id, sorted(tags & wanted)))
    return overlaps
