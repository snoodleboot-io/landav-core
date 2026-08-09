"""Load a directory of CSVs into one frame."""

import pandas as pd


def load_all(paths):
    """Every concat copies the whole accumulated frame."""
    frame = pd.DataFrame()
    for path in paths:
        frame = pd.concat([frame, pd.read_csv(path)])  # LANDAV: LAV009 anchor=pd.concat([frame, pd.read_csv(path)])
    return frame
