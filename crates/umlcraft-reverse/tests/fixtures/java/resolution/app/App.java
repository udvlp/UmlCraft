package app;

import java.util.*;
import java.io.File;
import lib.Helper;
import lib.missing.Gone;
import static java.lang.Math.max;

public class App<T> extends Base {
    // Template parameter of the class.
    private T value;
    // Template parameter of the operation shadows nothing here.
    public <E> E first(List<E> list) {
        return null;
    }
    // Member type, also an inherited one.
    private Inner inner;
    private Shared shared;
    // Single-type import of a known class, and of one the sources do not declare.
    private Helper helper;
    private Gone gone;
    // Own package before on-demand imports.
    private Base base;
    // On-demand import of the JDK, the implicit java.lang, and an explicit library type.
    private Date date;
    private String name;
    private File file;
    // Qualified names: from the root, through a type, and unknown.
    private lib.Helper qualified;
    private Map.Entry<String, App> entry;
    private com.acme.Thing thing;
    // Unknown simple name: a placeholder in the own package.
    private Unknown unknown;

    class Inner {
    }
}

class Base {
    static class Shared {
    }
}
