"""Classify HTTP requests as safe or unsafe."""


def safe_requests(requests):
    """Four pointer comparisons against interned strings.

    Hashing the method and probing a set costs more than the four identity
    checks a short list of literals does, which is why the same test written
    with a tuple is not reported either.
    """
    safe = []
    for request in requests:
        if request.method in ["GET", "HEAD", "OPTIONS", "TRACE"]:
            safe.append(request)
    return safe
