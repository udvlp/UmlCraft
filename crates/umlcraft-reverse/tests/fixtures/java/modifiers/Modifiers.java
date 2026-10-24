package modifiers;

public abstract strictfp class Modifiers {
    public int a;
    protected int b;
    int c;
    private int d;
    public static final int MAX = 10;
    private transient volatile Object cache;

    public Modifiers() {
    }

    protected abstract void run();

    public final synchronized void stop() {
    }

    public static native long now();

    private static int counter() {
        return 0;
    }
}

final class Leaf {
}

interface Contract {
    int LIMIT = 3;

    void call();

    default void twice() {
        call();
        call();
    }

    static Contract none() {
        return null;
    }

    private void helper() {
    }

    class Holder {
    }
}

enum Level {
    LOW, HIGH;

    Level() {
    }
}
