"""Join events to users, the shape the rule is asking for."""


def match_users(events, users):
    """The index is built once."""
    by_id = {user.id: user for user in users}
    hits = []
    for event in events:
        user = by_id.get(event.user_id)
        if user is not None:
            hits.append((event, user))
    return hits
