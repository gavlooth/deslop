def render(batch):
    left = expensive_transform(preprocess(batch))
    right = expensive_transform(preprocess(batch))
    return combine(left, right)
