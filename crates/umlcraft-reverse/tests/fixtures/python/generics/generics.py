from typing import Generic, Protocol, TypeVar

T = TypeVar("T")
K = TypeVar("K")


class Box(Generic[T]):
    item: T


class Pair(Generic[K, T]):
    key: K
    value: T


class Reader(Protocol[T]):
    def read(self) -> T: ...


class Modern[T, *Ts]:
    item: T

    def first[U](self, u: U) -> U: ...


class Comparable(Protocol):
    def compare(self, other: object) -> int: ...


class Sorted[T: Comparable]:
    items: list[T]

    def pick[V: Comparable](self, v: V) -> V: ...
