"""Maintain a top-N leaderboard."""


def score(event):
    """Placeholder ranking key."""
    return event.score


def top_by_score(events, limit):
    """The list is re-sorted on every single append."""
    ranked = []
    for event in events:
        ranked.append(event)
        ranked.sort(key=score, reverse=True)  # LANDAV: LAV006 anchor=ranked.sort(key=score, reverse=True)
        if len(ranked) > limit:
            ranked.pop()
    return ranked
