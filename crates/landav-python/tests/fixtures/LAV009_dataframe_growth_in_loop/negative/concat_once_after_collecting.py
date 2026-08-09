"""Load a directory of CSVs, the linear way."""

import pandas as pd


def load_all(paths):
    """One concat over a list of frames."""
    frames = [pd.read_csv(path) for path in paths]
    return pd.concat(frames, ignore_index=True)
