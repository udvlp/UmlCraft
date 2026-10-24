package javadoc;

/* A plain comment is no documentation. */
/**
 * A documented class.
 *
 * It has two paragraphs.
 *   @author Ulf
 */
// A line comment between the Javadoc and the class.
public class Documented {
    /** The value. */
    private int value;

    /**
     * Sets the value.
     * @param value the new value
     */
    public void set(int value) {
        this.value = value;
    }

    /* Not Javadoc. */
    public int get() {
        return value;
    }

    /** Only the last one counts. */
    /** This one. */
    public void reset() {
    }
}

/** Constants. */
enum Unit {
    /** Metres. */
    METRE,
    /** Seconds. */
    SECOND
}
