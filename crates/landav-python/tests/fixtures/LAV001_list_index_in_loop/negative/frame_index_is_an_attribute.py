"""Summarise a dataframe column by column."""


def column_summaries(frame):
    """``frame.index`` is an attribute lookup, not a linear search."""
    summaries = []
    for column in frame.columns:
        summaries.append((column, frame[column].sum(), frame.index.name))
    return summaries
