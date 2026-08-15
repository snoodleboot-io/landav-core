"""Index a stream of batches, one tag set per batch."""

_DEFAULT_TAGS = ("core", "indexed")


def _collect_tags(record, tags):
    """Adds this record's tags to ``tags`` in place."""
    tags.update(record.tags)


def index_batches(batches, sink):
    """``tags`` must be fresh for every batch, and the mutation is a call away.

    Nothing in this loop names ``tags.add`` or ``tags.update`` directly — the
    helper does it. Hoisting the build above the loop would leak every batch's
    tags into the next one, which changes the output rather than the cost.
    """
    for batch in batches:
        tags = set(_DEFAULT_TAGS)
        for record in batch:
            _collect_tags(record, tags)
        sink.write(sorted(tags))
