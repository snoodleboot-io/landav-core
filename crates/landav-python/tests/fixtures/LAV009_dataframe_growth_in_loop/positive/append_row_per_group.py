"""Summarise groups into a frame, one row at a time."""

import pandas as pd


def summarise(groups):
    """``DataFrame.append`` reallocates the frame on every call."""
    out = pd.DataFrame(columns=["group", "total"])
    for name, rows in groups:
        row = {"group": name, "total": rows.total.sum()}
        out = out.append(row, ignore_index=True)  # LANDAV: LAV009 anchor=out.append(row, ignore_index=True)
    return out
