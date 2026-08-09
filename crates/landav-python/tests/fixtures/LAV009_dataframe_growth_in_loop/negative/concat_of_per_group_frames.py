"""Widen each group with a lookup frame."""

import pandas as pd


def widen(frame, lookup):
    """``merged`` is discarded each iteration; nothing accumulates."""
    results = []
    for name, group in frame.groupby("kind"):
        merged = pd.concat([group, lookup[name]], axis=1)
        results.append(merged)
    return results
