"""Drop system accounts from a user list."""

BANNED = ["root", "admin", "daemon", "operator", "backup", "www-data", "nobody", "sync"]


def strip_system_users(users):
    """``BANNED`` is a list, so every check walks all eight entries."""
    kept = []
    for user in users:
        if user.name in BANNED:  # LANDAV: LAV002 anchor=user.name in BANNED
            continue
        kept.append(user)
    return kept
