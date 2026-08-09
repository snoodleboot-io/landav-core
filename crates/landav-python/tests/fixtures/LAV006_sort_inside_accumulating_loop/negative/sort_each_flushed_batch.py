"""Write records to a sink in sorted, fixed-size batches."""

_BATCH = 500


def _key(record):
    return record.timestamp


def write_batches(records, sink):
    """Each sort sees at most ``_BATCH`` records, and the list is reset after.

    The total is O(n log _BATCH), not O(n^2 log n). "Sort once after the loop"
    would change the output: the sink is meant to receive batches ordered
    within themselves, not one globally sorted run.
    """
    batch = []
    for record in records:
        batch.append(record)
        if len(batch) == _BATCH:
            sink.write(sorted(batch, key=_key))
            batch = []
    if batch:
        sink.write(sorted(batch, key=_key))
