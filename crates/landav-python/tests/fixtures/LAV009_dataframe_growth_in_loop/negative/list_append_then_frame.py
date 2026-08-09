"""Summarise groups by collecting dicts first."""

import pandas as pd


def summarise(groups):
    """``list.append`` is amortised O(1); the frame is built once."""
    records = []
    for name, rows in groups:
        records.append({"group": name, "total": rows.total.sum()})
    return pd.DataFrame.from_records(records)
