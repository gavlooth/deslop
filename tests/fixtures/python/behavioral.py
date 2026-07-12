from functools import wraps


def traced(function):
    @wraps(function)
    async def wrapper(*args, **kwargs):
        return await function(*args, **kwargs)

    return wrapper


class Service:
    @traced
    async def process(self, values):
        def normalize(value):
            return value.strip()

        return [normalize(value) for value in values]
