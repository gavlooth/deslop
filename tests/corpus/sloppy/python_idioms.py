def sloppy(value, items, data, key):
    if value == None:
        return []
    for idx in range(len(items)):
        print(items[idx])
    if key in data.keys():
        return list([item for item in items])
    return items
