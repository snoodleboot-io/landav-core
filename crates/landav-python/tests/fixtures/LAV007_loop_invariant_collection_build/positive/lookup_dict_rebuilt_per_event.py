"""Join events to users."""


def match_users(events, users):
    """The index does not depend on ``event``, so it is built once too often."""
    hits = []
    for event in events:
        by_id = {user.id: user for user in users}  # LANDAV: LAV007 anchor={user.id: user for user in users}
        if event.user_id in by_id:
            hits.append((event, by_id[event.user_id]))
    return hits
