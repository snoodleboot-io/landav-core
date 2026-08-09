"""Running median over a growing window."""


def running_medians(samples):
    """``sorted`` copies and sorts the whole window every iteration."""
    window = []
    medians = []
    for sample in samples:
        window.append(sample)
        ordered = sorted(window)  # LANDAV: LAV006 anchor=sorted(window)
        medians.append(ordered[len(ordered) // 2])
    return medians
