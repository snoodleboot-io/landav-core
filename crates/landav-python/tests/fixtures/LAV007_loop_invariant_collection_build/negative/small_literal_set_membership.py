"""Select finished jobs."""


def finished(jobs):
    """A literal set in a comparison is constant-folded; nothing is rebuilt."""
    out = []
    for job in jobs:
        if job.state in {"done", "failed"}:
            out.append(job)
    return out
