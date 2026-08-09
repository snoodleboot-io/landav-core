"""Label records from an optional metadata key."""


def labels(records):
    """``dict.get`` with a default expresses this without the unwind."""
    out = []
    for record in records:
        try:  # LANDAV: LAV010 anchor=try:
            label = record.meta["label"]
        except KeyError:
            label = "unknown"
        out.append(label)
    return out
