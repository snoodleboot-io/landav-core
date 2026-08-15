"""Leaderboard built the linearithmic way."""


def top_by_score(events, limit):
    """Collect everything, then sort once."""
    ranked = []
    for event in events:
        ranked.append(event)
    ranked.sort(key=lambda event: event.score, reverse=True)
    return ranked[:limit]
