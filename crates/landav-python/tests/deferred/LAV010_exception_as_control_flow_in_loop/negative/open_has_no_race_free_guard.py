"""Read a batch of files, recording the ones that fail."""


def read_all(paths):
    """There is no non-racy guard for ``open``; ``os.path.exists`` is a TOCTOU bug.

    The handler is the only correct way to express this, so the rule must
    stay silent no matter how often the exception is taken.
    """
    results = []
    failures = []
    for path in paths:
        try:
            with open(path, "rb") as handle:
                payload = handle.read()
        except OSError as exc:
            failures.append((path, exc))
            continue
        results.append(payload)
    return results, failures
