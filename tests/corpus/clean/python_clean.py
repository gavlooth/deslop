def clean(value, items, data, key):
    if value is None:
        return []
    for idx, item in enumerate(items):
        print(idx, item)
    if key in data:
        return [item for item in items]
    return list(items)
