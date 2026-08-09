"""Depth-first walk over a tree."""


def walk(root):
    """``pop()`` with no index is an O(1) stack pop."""
    stack = [root]
    visited = []
    while stack:
        node = stack.pop()
        visited.append(node)
        stack.extend(node.children)
    return visited
