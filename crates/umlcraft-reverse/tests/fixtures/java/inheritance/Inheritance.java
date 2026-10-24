package inheritance;

import java.io.Serializable;

interface Named {
    String name();
}

interface Labeled extends Named, Comparable<Labeled> {
}

abstract class Base implements Serializable {
}

public class Derived extends Base implements Labeled, Cloneable {
    public String name() {
        return "";
    }

    public int compareTo(Labeled other) {
        return 0;
    }
}

enum Kind implements Named {
    ONE;

    public String name2() {
        return "";
    }
}
