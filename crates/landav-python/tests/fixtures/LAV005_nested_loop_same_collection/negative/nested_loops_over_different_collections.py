"""Flatten users to (user, role) pairs."""


def role_pairs(users):
    """Total work is the number of roles, not users times roles."""
    pairs = []
    for user in users:
        for role in user.roles:
            pairs.append((user.name, role.name))
    return pairs
