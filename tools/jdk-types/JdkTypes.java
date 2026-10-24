import java.io.IOException;
import java.io.PrintWriter;
import java.lang.module.ModuleReader;
import java.lang.module.ModuleReference;
import java.lang.module.ResolvedModule;
import java.lang.reflect.Modifier;
import java.lang.reflect.TypeVariable;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.SortedSet;
import java.util.TreeSet;

/// Writes the table of public JDK types that UmlCraft's Java import resolves names against
/// (`crates/umlcraft-reverse/src/java/jdk.txt`), one type per line, tab separated:
///
///     package  type  kind  type-parameters  container
///
/// - type: the name inside the package, nested types joined by `.` (`Map.Entry`);
/// - kind: `c` class (records too), `i` interface (annotation types too), `e` enum;
/// - type-parameters: their names, comma separated, as Astah names them (`E`; `K,V`);
/// - container: `-`, or for collections `C` followed by `o` (ordered) and `u` (unique), `M` for
///   maps (key, then value). Astah does not unwrap `Optional`.
///
/// Covers the packages the `java.*` modules export to everyone. Run it with any JDK in source
/// mode, e.g. Astah's: `java JdkTypes.java <output>`.
public final class JdkTypes {
    public static void main(String[] args) throws IOException {
        Set<String> lines = new TreeSet<>();
        for (ResolvedModule resolved : ModuleLayer.boot().configuration().modules()) {
            String module = resolved.name();
            if (!module.startsWith("java.")) {
                continue;
            }
            Module m = ModuleLayer.boot().findModule(module).orElseThrow();
            Set<String> exported = new TreeSet<>();
            for (var export : m.getDescriptor().exports()) {
                if (!export.isQualified()) {
                    exported.add(export.source());
                }
            }
            ModuleReference reference = resolved.reference();
            try (ModuleReader reader = reference.open()) {
                for (String resource : (Iterable<String>) reader.list()::iterator) {
                    if (!resource.endsWith(".class") || resource.endsWith("module-info.class")
                            || resource.endsWith("package-info.class")) {
                        continue;
                    }
                    String binary = resource.substring(0, resource.length() - 6).replace('/', '.');
                    int dot = binary.lastIndexOf('.');
                    String pkg = binary.substring(0, dot);
                    if (!exported.contains(pkg)) {
                        continue;
                    }
                    String line = describe(binary, pkg);
                    if (line != null) {
                        lines.add(line);
                    }
                }
            }
        }
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0]), StandardCharsets.UTF_8))) {
            for (String line : lines) {
                out.print(line);
                out.print('\n');
            }
        }
        System.out.println(lines.size() + " types");
    }

    private static String describe(String binary, String pkg) {
        Class<?> c;
        try {
            c = Class.forName(binary, false, ClassLoader.getPlatformClassLoader());
        } catch (Throwable e) {
            return null;
        }
        // Public all the way out; no anonymous or local classes.
        for (Class<?> o = c; o != null; o = o.getDeclaringClass()) {
            if (!Modifier.isPublic(o.getModifiers()) || o.isAnonymousClass() || o.isLocalClass()
                    || o.isSynthetic()) {
                return null;
            }
        }
        String name = binary.substring(pkg.length() + 1).replace('$', '.');
        String kind = c.isEnum() ? "e" : c.isInterface() ? "i" : "c";
        List<String> params = new ArrayList<>();
        for (TypeVariable<?> v : c.getTypeParameters()) {
            params.add(v.getName());
        }
        String container = "-";
        if (Collection.class.isAssignableFrom(c) && params.size() >= 1) {
            container = "C";
            if (List.class.isAssignableFrom(c) || SortedSet.class.isAssignableFrom(c)) {
                container += "o";
            }
            if (Set.class.isAssignableFrom(c)) {
                container += "u";
            }
        } else if ((Map.class.isAssignableFrom(c) || java.util.Dictionary.class.isAssignableFrom(c))
                && params.size() >= 2) {
            // Astah treats the old `Dictionary` like a map.
            container = "M";
        }
        // Astah's placeholders name their parameters by count, not as the JDK does:
        // `Comparable<E>`, `Optional<E>`, `Map<K, V>`.
        if (params.size() == 1) {
            params = List.of("E");
        } else if (params.size() == 2) {
            params = List.of("K", "V");
        }
        return pkg + "\t" + name + "\t" + kind + "\t" + String.join(",", params) + "\t" + container;
    }
}
