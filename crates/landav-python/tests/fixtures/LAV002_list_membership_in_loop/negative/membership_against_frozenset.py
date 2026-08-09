"""Drop system accounts, the constant-time way."""

BANNED = frozenset({"root", "admin", "daemon", "operator", "backup", "www-data", "nobody"})


def strip_system_users(users):
    """A hash lookup per user."""
    kept = []
    for user in users:
        if user.name in BANNED:
            continue
        kept.append(user)
    return kept
