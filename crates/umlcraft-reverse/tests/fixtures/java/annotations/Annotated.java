package annotations;

@Deprecated
@SuppressWarnings({"unchecked", "rawtypes"})
public class Annotated {
    @Deprecated
    private int old;

    @Override
    public String toString() {
        return "";
    }

    public void take(@SuppressWarnings("unused") final int value) {
    }
}
