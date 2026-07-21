import os

DEFAULTS = {"THRESHOLD": 0.5}


def load_config():
    config = dict(DEFAULTS)
    config["THRESHOLD"] = os.environ["THRESHOLD_V2"]
    return config
