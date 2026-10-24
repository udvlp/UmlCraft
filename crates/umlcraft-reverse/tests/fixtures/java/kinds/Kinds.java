package kinds;

public class Kinds {
    public interface Shape {
        double area();
    }

    public enum Color {
        RED, GREEN, BLUE;

        public Color next() {
            return values()[(ordinal() + 1) % values().length];
        }
    }

    public record Point(int x, int y) {
        public Point {
            if (x < 0) throw new IllegalArgumentException();
        }
    }

    public @interface Marker {
        String value() default "";
        int priority();
    }

    static class Nested {
        class Deeper {
        }
    }
}
