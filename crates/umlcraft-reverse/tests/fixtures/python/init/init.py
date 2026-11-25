class Point:
    z: float = 0.0

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y: float = y
        self.z = 1.0
        self._cache = None
        if x:
            self.hidden = 1
        other = self
        other.not_mine = 2


class Late:
    def __init__(self) -> None:
        self.total = 0

    total: int
