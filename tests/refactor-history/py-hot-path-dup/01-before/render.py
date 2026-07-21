def render(batch):
    return combine(expensive_transform(preprocess(batch)))
