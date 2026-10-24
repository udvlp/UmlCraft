package generics;

import java.util.List;
import java.util.Map;

public class Generics<T extends Number & Comparable<T>, U> {
    private Map<String, List<Foo>> index;
    private List<? extends T> upper;
    private List<? super Integer> lower;
    private List<?> any;
    private Map.Entry<String, U> entry;

    public <K extends Comparable<K>> K max(List<K> values) {
        return null;
    }

    public static <E> void fill(List<? super E> target, E value) {
    }
}

class Foo {
}
