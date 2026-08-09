"""Validate requested machine profiles against the catalogue."""

KNOWN_PROFILES = [
    {"name": "small", "cpu": 1, "memory_gb": 2},
    {"name": "medium", "cpu": 2, "memory_gb": 8},
    {"name": "large", "cpu": 8, "memory_gb": 32},
    {"name": "xlarge", "cpu": 32, "memory_gb": 128},
]


def reject_unknown(requests):
    """A dict is unhashable, so there is no set to move to.

    ``in`` against a list of dicts compares four dicts; a ``set`` or ``dict`` of
    them cannot be constructed at all. The catalogue is also a fixed four
    entries, so the comparison is constant work per request.
    """
    rejected = []
    for request in requests:
        if request.profile not in KNOWN_PROFILES:
            rejected.append(request)
    return rejected
