"""Normalise text through a fixed pipeline of substitutions."""

import re

_SUBSTITUTIONS = {
    r"\s+": " ",
    " ": " ",
    "[‘’]": "'",
    "[“”]": '"',
}


def normalise(raw):
    """Every rule is applied exactly once; this is a pipeline, not a fixpoint.

    The loop runs once per substitution — four times — and each pass is linear
    in the text, so the whole function is linear. Rebinding ``text`` is how a
    pipeline threads its value; nothing here iterates until the pattern stops
    matching.
    """
    text = raw
    for pattern, replacement in _SUBSTITUTIONS.items():
        text = re.sub(pattern, replacement, text)
    return text
