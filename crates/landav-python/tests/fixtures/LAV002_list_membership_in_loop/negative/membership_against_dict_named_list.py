"""Annotate events from a history index."""


def annotate(events, history):
    """``index_list`` is a dict: ``in`` is a hash lookup despite the name."""
    index_list = {entry.key: entry for entry in history}
    annotated = []
    for event in events:
        if event.key in index_list:
            annotated.append((event, index_list[event.key]))
    return annotated
