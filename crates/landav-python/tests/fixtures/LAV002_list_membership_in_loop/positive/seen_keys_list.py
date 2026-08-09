"""Find events that are not already in the history."""


def unseen(events, history):
    """``seen`` is a list, so ``not in`` is a scan of the whole history."""
    seen = [entry.key for entry in history]
    fresh = []
    for event in events:
        if event.key not in seen:  # LANDAV: LAV002 anchor=event.key not in seen
            fresh.append(event)
    return fresh
