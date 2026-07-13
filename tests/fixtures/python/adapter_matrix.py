# @generated
from dataclasses import dataclass


@generated
@dataclass
class Report:
    value: int


async def compute(π: int, values: list[int]) -> int:
    # line comment
    total = π * 2
    for value in values:
        if value > 0:
            total += value

    match total:
        case 0:
            return 0
        case _:
            print total

    exec "legacy = True"
    return await helpers.normalize(total)
