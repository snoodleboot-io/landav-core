"""Materialise the entities a tokeniser reported as ``(start, end)`` offsets."""


def entity_texts(text, spans):
    """Each span is copied once, and the spans do not overlap.

    The total copied is the length of ``text``, so the loop is linear. The
    slice bounds move with the loop variable because that is what an offset
    *is*; the span itself does not grow. There is no index to keep and no
    memoryview to take — ``str`` slicing is how you get the substring.
    """
    entities = []
    for start, end in spans:
        entities.append(text[start:end])
    return entities
