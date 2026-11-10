"""Zeller-Hildebrandt ddmin: returns a 1-minimal failing subsequence (not globally minimal)."""


def ddmin(items, fails):
    items = list(items)
    if not fails(items):
        raise ValueError("input does not fail")
    n = 2
    while len(items) >= 2:
        size = -(-len(items) // n)
        chunks = [items[i:i + size] for i in range(0, len(items), size)]
        for i, c in enumerate(chunks):
            comp = [x for j, ch in enumerate(chunks) if j != i for x in ch]
            if fails(c):
                items, n = c, 2
                break
            if fails(comp):
                items, n = comp, max(n - 1, 2)
                break
        else:
            if n >= len(items):
                break
            n = min(n * 2, len(items))
    return items
