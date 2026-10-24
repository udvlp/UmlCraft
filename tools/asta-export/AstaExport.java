// Builds an Astah project (.asta) from an UmlCraft project (.ucraft) through the official Astah
// API. Run with the JRE that comes with Astah, in source-file mode:
//
//   java -cp astah-api.jar;astah-pro.jar AstaExport.java project.ucraft layout.json out.asta report.txt
//
// `layout.json` carries what UmlCraft derives instead of storing it: node sizes grown to their
// content and edges as drawn. Sequence diagrams are built from the geometry stored in the
// project, which follows Astah's. The report lists what Astah's
// API cannot express, one `dropped <key> <count>` line per kind, `problem <text>` lines for
// single failures and an `error <text>` line if the export failed as a whole.
//
// This file only uses the public API in `com.change_vision.jude.api.inf`; see README.md.

import java.awt.geom.Point2D;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

import com.change_vision.jude.api.inf.AstahAPI;
import com.change_vision.jude.api.inf.editor.ActivityDiagramEditor;
import com.change_vision.jude.api.inf.editor.BasicDiagramEditor;
import com.change_vision.jude.api.inf.editor.BasicModelEditor;
import com.change_vision.jude.api.inf.editor.ClassDiagramEditor;
import com.change_vision.jude.api.inf.editor.SequenceDiagramEditor;
import com.change_vision.jude.api.inf.editor.StateMachineDiagramEditor;
import com.change_vision.jude.api.inf.editor.StructureDiagramEditor;
import com.change_vision.jude.api.inf.editor.TransactionManager;
import com.change_vision.jude.api.inf.editor.UseCaseDiagramEditor;
import com.change_vision.jude.api.inf.editor.UseCaseModelEditor;
import com.change_vision.jude.api.inf.exception.InvalidEditingException;
import com.change_vision.jude.api.inf.model.IActivityDiagram;
import com.change_vision.jude.api.inf.model.IAssociation;
import com.change_vision.jude.api.inf.model.IAttribute;
import com.change_vision.jude.api.inf.model.IClass;
import com.change_vision.jude.api.inf.model.ICombinedFragment;
import com.change_vision.jude.api.inf.model.IDependency;
import com.change_vision.jude.api.inf.model.IDiagram;
import com.change_vision.jude.api.inf.model.IElement;
import com.change_vision.jude.api.inf.model.IEnumeration;
import com.change_vision.jude.api.inf.model.IFlow;
import com.change_vision.jude.api.inf.model.IInstanceSpecification;
import com.change_vision.jude.api.inf.model.IInteractionOperand;
import com.change_vision.jude.api.inf.model.IInteractionUse;
import com.change_vision.jude.api.inf.model.ILifeline;
import com.change_vision.jude.api.inf.model.IMessage;
import com.change_vision.jude.api.inf.model.IModel;
import com.change_vision.jude.api.inf.model.IMultiplicityRange;
import com.change_vision.jude.api.inf.model.INamedElement;
import com.change_vision.jude.api.inf.model.IOperation;
import com.change_vision.jude.api.inf.model.IPackage;
import com.change_vision.jude.api.inf.model.IPartition;
import com.change_vision.jude.api.inf.model.IParameter;
import com.change_vision.jude.api.inf.model.ISequenceDiagram;
import com.change_vision.jude.api.inf.model.IState;
import com.change_vision.jude.api.inf.model.IStateMachineDiagram;
import com.change_vision.jude.api.inf.model.ITransition;
import com.change_vision.jude.api.inf.model.IUsage;
import com.change_vision.jude.api.inf.model.IUseCase;
import com.change_vision.jude.api.inf.presentation.ILinkPresentation;
import com.change_vision.jude.api.inf.presentation.INodePresentation;
import com.change_vision.jude.api.inf.presentation.IPresentation;
import com.change_vision.jude.api.inf.project.ProjectAccessor;

@SuppressWarnings("unchecked")
public final class AstaExport {
    private static final String DEFAULT_FILL = "#FFFFCC";

    public static void main(String[] args) {
        if (args.length != 4) {
            System.err.println("usage: AstaExport <project.ucraft> <layout.json> <out.asta> <report.txt>");
            System.exit(2);
        }
        List<String> report = new ArrayList<>();
        int status = 0;
        try {
            AstaExport export = new AstaExport(read(args[0]), read(args[1]));
            export.run(args[2]);
            export.dropped.forEach((key, count) -> report.add("dropped\t" + key + "\t" + count));
            export.problems.forEach(problem -> report.add("problem\t" + problem));
        } catch (Throwable t) {
            // Astah swallows exceptions that escape `main`, so they are reported here.
            report.add("error\t" + describe(t));
            status = 1;
        }
        try {
            Files.write(Paths.get(args[3]), report, StandardCharsets.UTF_8);
        } catch (IOException e) {
            status = 3;
        }
        // The API starts AWT threads that would keep the VM alive.
        System.exit(status);
    }

    private static Object read(String path) throws IOException {
        return new Json(new String(Files.readAllBytes(Paths.get(path)), StandardCharsets.UTF_8)).document();
    }

    private static String describe(Throwable t) {
        String message = t.getMessage() == null ? "" : t.getMessage();
        String where = t.getStackTrace().length > 0 ? " (" + t.getStackTrace()[0] + ")" : "";
        // Where in this tool it happened.
        for (StackTraceElement frame : t.getStackTrace()) {
            if (frame.getClassName().startsWith("AstaExport")) {
                where += " at line " + frame.getLineNumber();
                break;
            }
        }
        return (t.getClass().getSimpleName() + ": " + message + where).replaceAll("\\s+", " ");
    }

    // ----- Input ------------------------------------------------------------------------------

    private final String root;
    private final Map<String, Map<String, Object>> elements = new HashMap<>();
    private final Map<String, String> owners = new HashMap<>();
    private final Map<String, List<Map<String, Object>>> views;
    private final Map<String, Object> layouts;

    // ----- Output -----------------------------------------------------------------------------

    private ProjectAccessor accessor;
    private BasicModelEditor basic;
    private UseCaseModelEditor useCases;
    /// UmlCraft element id → Astah model element.
    private final Map<String, Object> created = new HashMap<>();
    private final List<String> features = new ArrayList<>();
    private final List<String> relationships = new ArrayList<>();
    private final List<String> diagrams = new ArrayList<>();
    private final Set<String> shownComments = new HashSet<>();
    /// Associations whose ends the API created in the other order.
    private final Set<String> swappedEnds = new HashSet<>();
    /// Default names handed out to unnamed control nodes, by stem.
    private final Map<String, Integer> controlNames = new HashMap<>();
    /// UmlCraft interaction → the Astah sequence diagram that brought it along.
    private final Map<String, ISequenceDiagram> sequenceDiagrams = new HashMap<>();
    /// Interaction uses and the UmlCraft interaction they refer to, set once all diagrams exist.
    private final Map<IInteractionUse, String> references = new LinkedHashMap<>();
    private final TreeMap<String, Integer> dropped = new TreeMap<>();
    private final List<String> problems = new ArrayList<>();

    private AstaExport(Object project, Object layout) {
        Map<String, Object> file = (Map<String, Object>) project;
        if (!"umlcraft".equals(file.get("format"))) {
            throw new IllegalArgumentException("not an UmlCraft project file");
        }
        root = (String) file.get("root");
        for (Object e : list(file.get("elements"))) {
            Map<String, Object> element = (Map<String, Object>) e;
            String id = (String) element.get("id");
            elements.put(id, element);
            for (Object child : list(element.get("children"))) {
                owners.put((String) child, id);
            }
        }
        Map<String, List<Map<String, Object>>> v = (Map<String, List<Map<String, Object>>>) file.get("views");
        views = v == null ? new HashMap<>() : v;
        Map<String, Object> l = layout == null ? null : (Map<String, Object>) ((Map<String, Object>) layout).get("diagrams");
        layouts = l == null ? new HashMap<>() : l;
    }

    private void run(String out) throws Exception {
        accessor = AstahAPI.getAstahAPI().getProjectAccessor();
        accessor.create(out);
        try {
            TransactionManager.beginTransaction();
            try {
                build();
                TransactionManager.endTransaction();
            } catch (Throwable t) {
                TransactionManager.abortTransaction();
                throw t;
            }
            accessor.save();
        } finally {
            accessor.close();
        }
    }

    private void build() throws Exception {
        basic = accessor.getModelEditorFactory().getBasicModelEditor();
        useCases = accessor.getModelEditorFactory().getUseCaseModelEditor();
        IModel project = accessor.getProject();
        Map<String, Object> model = elements.get(root);
        if (!name(model).isEmpty()) {
            project.setName(name(model));
        }
        common(root, project, Set.of());
        created.put(root, project);

        structure(root);
        for (String id : features) {
            attempt(id, () -> {
                feature(id);
                return null;
            });
        }
        for (String id : relationships) {
            attempt(id, () -> {
                relationship(id);
                return null;
            });
        }
        for (String id : diagrams) {
            attempt(id, () -> diagram(id));
        }
        // Astah's interaction uses refer to sequence diagrams.
        for (Map.Entry<IInteractionUse, String> r : references.entrySet()) {
            ISequenceDiagram target = sequenceDiagrams.get(r.getValue());
            if (target != null) {
                r.getKey().setSequenceDiagram(target);
            } else {
                drop("interactionUseReference");
            }
        }
        for (Map<String, Object> e : elements.values()) {
            if ("comment".equals(type(e)) && !shownComments.contains(e.get("id"))) {
                // Astah keeps comments as notes on diagrams only.
                drop("comment");
            }
            if ("instance".equals(type(e)) && !created.containsKey(e.get("id"))) {
                // The API creates instance specifications on class diagrams only.
                drop("instance");
            }
        }
    }

    // ----- Packages, classifiers and their parts -----------------------------------------------

    /// Creates packages, classifiers, extension points and literals in tree order; features,
    /// relationships and diagrams are collected for later, once every type exists.
    private void structure(String id) {
        Object owner = created.get(id);
        for (String child : children(id)) {
            Map<String, Object> e = elements.get(child);
            String type = type(e);
            switch (type) {
                case "package", "class", "interface", "enumeration", "dataType", "actor", "useCase",
                        "extensionPoint", "enumerationLiteral", "component", "artifact", "node" -> {
                    Object made = attempt(child, () -> element(child, type, owner));
                    if (made != null) {
                        created.put(child, made);
                        structure(child);
                    }
                }
                case "attribute", "operation" -> features.add(child);
                case "association", "generalization", "realization", "dependency", "include", "extend" ->
                    relationships.add(child);
                case "diagram" -> diagrams.add(child);
                case "interaction" -> {
                    List<String> own = new ArrayList<>();
                    for (String d : children(child)) {
                        if ("diagram".equals(type(elements.get(d)))) {
                            own.add(d);
                        }
                    }
                    if (own.isEmpty()) {
                        drop("interaction");
                    }
                    diagrams.addAll(own);
                }
                case "activity", "stateMachine" -> {
                    List<String> own = new ArrayList<>();
                    for (String d : children(child)) {
                        if ("diagram".equals(type(elements.get(d)))) {
                            own.add(d);
                        }
                    }
                    if (own.isEmpty()) {
                        drop(type);
                    } else if (own.size() > 1) {
                        // Astah's activities and state machines have exactly one diagram.
                        drop(type + "Diagram");
                    }
                    diagrams.addAll(own.subList(0, Math.min(1, own.size())));
                }
                // Comments and instances are created with their presentations on diagrams.
                case "comment", "instance" -> {
                }
                default -> drop(type);
            }
        }
    }

    private Object element(String id, String type, Object owner) throws Exception {
        Map<String, Object> e = elements.get(id);
        Object made = switch (type) {
            case "package" -> named(id, n -> basic.createPackage((IPackage) owner, n));
            case "class", "dataType" -> named(id, n -> owner instanceof IPackage p
                ? basic.createClass(p, n) : basic.createClass((IClass) owner, n));
            case "interface" -> named(id, n -> owner instanceof IPackage p
                ? basic.createInterface(p, n) : basic.createInterface((IClass) owner, n));
            case "enumeration" -> named(id, n -> basic.createEnumeration(nearestPackage(id), n));
            case "component" -> named(id, n -> basic.createComponent(nearestPackage(id), n));
            case "artifact" -> named(id, n -> basic.createArtifact(nearestPackage(id), n));
            case "node" -> named(id, n -> basic.createNode(nearestPackage(id), n));
            case "actor" -> named(id, n -> useCases.createActor((IPackage) owner, n));
            case "useCase" -> named(id, n -> useCases.createUseCase((IPackage) owner, n));
            case "extensionPoint" -> named(id, n -> useCases.createExtensionPoint((IUseCase) owner, n));
            case "enumerationLiteral" -> named(id, n -> basic.createEnumerationLiteral((IEnumeration) owner, n));
            default -> throw new IllegalStateException(type);
        };
        if (made instanceof IClass c) {
            c.setAbstract(flag(e, "isAbstract"));
            c.setLeaf(flag(e, "isLeaf"));
            c.setActive(flag(e, "isActive"));
        }
        if ("dataType".equals(type)) {
            ((IElement) made).addStereotype("dataType");
        }
        if ("enumeration".equals(type) && !(owner instanceof IPackage)) {
            drop("nestedEnumeration");
        }
        if (Set.of("component", "artifact", "node").contains(type) && !(owner instanceof IPackage)) {
            // The API creates these in packages only.
            drop("nestedComponent");
        }
        common(id, (IElement) made, Set.of());
        return made;
    }

    private IPackage nearestPackage(String id) {
        for (String o = owners.get(id); o != null; o = owners.get(o)) {
            if (created.get(o) instanceof IPackage p) {
                return p;
            }
        }
        return (IPackage) created.get(root);
    }

    private void feature(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        if (!(created.get(owners.get(id)) instanceof IClass owner)) {
            drop(type(e));
            return;
        }
        if ("attribute".equals(type(e))) {
            Object t = type(e.get("typeRef"));
            IAttribute a = (IAttribute) named(id, n -> t instanceof IClass c
                ? basic.createAttribute(owner, n, c) : basic.createAttribute(owner, n, (String) t));
            multiplicity(a, (String) e.get("multiplicity"));
            if (!text(e, "defaultValue").isEmpty()) {
                a.setInitialValue(text(e, "defaultValue"));
            }
            a.setStatic(flag(e, "isStatic"));
            a.setDerived(flag(e, "isDerived"));
            a.setChangeable(!flag(e, "isReadOnly"));
            common(id, a, Set.of());
            created.put(id, a);
            return;
        }
        Object r = e.get("returnType") == null ? "void" : type(e.get("returnType"));
        IOperation op = (IOperation) named(id, n -> r instanceof IClass c
            ? basic.createOperation(owner, n, c) : basic.createOperation(owner, n, (String) r));
        op.setStatic(flag(e, "isStatic"));
        op.setAbstract(flag(e, "isAbstract"));
        if (flag(e, "isQuery")) {
            drop("query");
        }
        common(id, op, Set.of());
        created.put(id, op);
        for (String child : children(id)) {
            attempt(child, () -> parameter(op, child));
        }
    }

    private Object parameter(IOperation op, String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        if (!"parameter".equals(type(e))) {
            drop(type(e));
            return null;
        }
        Object t = type(e.get("typeRef"));
        IParameter p = t instanceof IClass c
            ? basic.createParameter(op, name(e), c) : basic.createParameter(op, name(e), (String) t);
        switch (text(e, "direction")) {
            case "out" -> p.setDirection("out");
            case "inOut" -> p.setDirection("inout");
            case "return" -> drop("returnParameter");
            default -> {
            }
        }
        if (e.get("multiplicity") != null || !text(e, "defaultValue").isEmpty()) {
            drop("parameterDetails");
        }
        common(id, p, Set.of());
        return p;
    }

    /// A type reference as the API wants it: a created classifier, or the type's name. The API
    /// turns a name that is neither a primitive nor a classifier into a new class. Astah has no
    /// untyped attributes or parameters, so a missing type becomes the primitive `void`.
    private Object type(Object ref) {
        if (ref instanceof Map<?, ?> m) {
            if (m.get("element") instanceof String target) {
                Object c = created.get(target);
                if (c instanceof IClass) {
                    return c;
                }
                return name(elements.get(target));
            }
            if (m.get("text") instanceof String text && !text.isBlank()) {
                return text;
            }
        }
        return "void";
    }

    private static void multiplicity(IAttribute a, String text) throws InvalidEditingException {
        if (text == null) {
            return;
        }
        int lower;
        int upper;
        String[] bounds = text.split("\\.\\.");
        if (bounds.length == 2) {
            lower = Integer.parseInt(bounds[0].trim());
            upper = bound(bounds[1]);
        } else if ("*".equals(text.trim())) {
            lower = 0;
            upper = IMultiplicityRange.UNLIMITED;
        } else {
            lower = Integer.parseInt(text.trim());
            upper = lower;
        }
        a.setMultiplicity(new int[][] { { lower, upper } });
    }

    private static int bound(String b) {
        return "*".equals(b.trim()) ? IMultiplicityRange.UNLIMITED : Integer.parseInt(b.trim());
    }

    // ----- Relationships ----------------------------------------------------------------------

    private void relationship(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        String name = name(e);
        Set<String> implied = Set.of();
        IElement made;
        switch (type(e)) {
            case "association" -> {
                List<Object> ends = list(e.get("ends"));
                Map<String, Object> end0 = (Map<String, Object>) ends.get(0);
                Map<String, Object> end1 = (Map<String, Object>) ends.get(1);
                IClass a = classifier(end0.get("participant"));
                IClass b = classifier(end1.get("participant"));
                IAssociation association =
                    basic.createAssociation(a, b, name, text(end0, "role"), text(end1, "role"));
                IAttribute[] members = association.getMemberEnds();
                boolean swapped = a != b && members[0].getType() != a;
                if (swapped) {
                    swappedEnds.add(id);
                }
                end(swapped ? members[1] : members[0], end0);
                end(swapped ? members[0] : members[1], end1);
                made = association;
            }
            case "generalization" -> made =
                basic.createGeneralization(classifier(e.get("specific")), classifier(e.get("general")), name);
            case "realization" -> made =
                basic.createRealization(classifier(e.get("client")), classifier(e.get("supplier")), name);
            case "dependency" -> {
                INamedElement client = named(e.get("client"));
                INamedElement supplier = named(e.get("supplier"));
                INamedElement madeClient;
                if (list(e.get("stereotypes")).contains("use")
                        && client instanceof IClass c && supplier instanceof IClass s) {
                    IUsage usage = basic.createUsage(c, s, name);
                    madeClient = usage.getClient();
                    made = usage;
                    implied = Set.of("use");
                } else {
                    IDependency dependency = basic.createDependency(supplier, client, name);
                    madeClient = dependency.getClient();
                    made = dependency;
                }
                if (madeClient != client) {
                    throw new IllegalStateException("dependency created the wrong way round");
                }
            }
            case "include" -> {
                made = useCases.createInclude(useCase(e.get("including")), useCase(e.get("addition")), name);
                implied = Set.of("include");
            }
            case "extend" -> {
                made = useCases.createExtend(useCase(e.get("extension")), useCase(e.get("extended")), name);
                implied = Set.of("extend");
                if (!text(e, "condition").isEmpty()) {
                    drop("extendCondition");
                }
                if (!list(e.get("extensionPoints")).isEmpty()) {
                    drop("extendExtensionPoints");
                }
            }
            default -> throw new IllegalStateException(type(e));
        }
        common(id, made, implied);
        created.put(id, made);
    }

    private void end(IAttribute end, Map<String, Object> e) throws InvalidEditingException {
        multiplicity(end, (String) e.get("multiplicity"));
        for (Object constraint : list(e.get("constraints"))) {
            basic.createConstraint(end, (String) constraint);
        }
        switch (text(e, "navigability")) {
            case "navigable" -> end.setNavigability("Navigable");
            case "nonNavigable" -> end.setNavigability("Non_Navigable");
            default -> {
            }
        }
        switch (text(e, "aggregation")) {
            case "shared" -> end.setAggregation();
            case "composite" -> end.setComposite();
            default -> {
            }
        }
        if (e.get("visibility") instanceof String v) {
            end.setVisibility(v);
        }
    }

    private IClass classifier(Object id) {
        if (created.get(id) instanceof IClass c) {
            return c;
        }
        throw new IllegalStateException("end " + id + " was not exported");
    }

    private IUseCase useCase(Object id) {
        if (created.get(id) instanceof IUseCase u) {
            return u;
        }
        throw new IllegalStateException("use case " + id + " was not exported");
    }

    private INamedElement named(Object id) {
        if (created.get(id) instanceof INamedElement n) {
            return n;
        }
        throw new IllegalStateException("end " + id + " was not exported");
    }

    // ----- Diagrams ---------------------------------------------------------------------------

    private Object diagram(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        String kind = text(e, "diagram");
        Object owner = created.get(owners.get(id));
        IDiagram diagram;
        switch (kind) {
            case "class", "useCase", "component", "deployment" -> {
                StructureDiagramEditor editor;
                if (!"useCase".equals(kind)) {
                    // The API has no editors for component and deployment diagrams; a class
                    // diagram shows the same elements.
                    if (!"class".equals(kind)) {
                        drop(kind + "Diagram");
                    }
                    ClassDiagramEditor cd = accessor.getDiagramEditorFactory().getClassDiagramEditor();
                    diagram = (IDiagram) named(id, n -> cd.createClassDiagram((INamedElement) owner, n));
                    editor = cd;
                } else {
                    UseCaseDiagramEditor ud = accessor.getDiagramEditorFactory().getUseCaseDiagramEditor();
                    diagram = (IDiagram) named(id, n -> ud.createUseCaseDiagram((INamedElement) owner, n));
                    editor = ud;
                }
                structureDiagram(id, editor);
            }
            case "sequence" -> {
                // A sequence diagram brings its own interaction, owned by the interaction's owner.
                String interaction = owners.get(id);
                Object interactionOwner = created.get(owners.get(interaction));
                SequenceDiagramEditor sd = accessor.getDiagramEditorFactory().getSequenceDiagramEditor();
                ISequenceDiagram d =
                    (ISequenceDiagram) named(id, n -> sd.createSequenceDiagram((INamedElement) interactionOwner, n));
                String interactionName = name(elements.get(interaction));
                if (!interactionName.isEmpty()) {
                    d.getInteraction().setName(interactionName);
                }
                common(interaction, d.getInteraction(), Set.of());
                sequenceDiagrams.put(interaction, d);
                // Message numbers and return values, as the diagram shows them.
                Map<String, Object> props = elements.get(id);
                d.setProperty("message_index_visibility",
                    Boolean.toString(!Boolean.TRUE.equals(props.get("hideMessageNumbers"))));
                d.setProperty("message_return_value_initial_visibility",
                    Boolean.toString(!Boolean.TRUE.equals(props.get("hideReturnValues"))));
                new Sequence(id, sd).build();
                diagram = d;
            }
            case "activity" -> {
                // An activity diagram brings its own activity, owned by the activity's owner.
                String activity = owners.get(id);
                Object activityOwner = created.get(owners.get(activity));
                ActivityDiagramEditor ad = accessor.getDiagramEditorFactory().getActivityDiagramEditor();
                IActivityDiagram d =
                    (IActivityDiagram) named(id, n -> ad.createActivityDiagram((INamedElement) activityOwner, n));
                String activityName = name(elements.get(activity));
                if (!activityName.isEmpty()) {
                    d.getActivity().setName(activityName);
                }
                common(activity, d.getActivity(), Set.of());
                activityDiagram(id, activity, ad);
                diagram = d;
            }
            case "stateMachine" -> {
                // A state machine diagram brings its own state machine, like an activity diagram.
                String machine = owners.get(id);
                Object machineOwner = created.get(owners.get(machine));
                StateMachineDiagramEditor sm = accessor.getDiagramEditorFactory().getStateMachineDiagramEditor();
                IStateMachineDiagram d =
                    (IStateMachineDiagram) named(id, n -> sm.createStatemachineDiagram((INamedElement) machineOwner, n));
                String machineName = name(elements.get(machine));
                if (!machineName.isEmpty()) {
                    d.getStateMachine().setName(machineName);
                }
                common(machine, d.getStateMachine(), Set.of());
                stateDiagram(id, machine, sm);
                diagram = d;
            }
            default -> throw new IllegalStateException(kind);
        }
        common(id, diagram, Set.of());
        return diagram;
    }

    /// Nodes in z-order, then edges and note anchors of a class or use case diagram.
    private void structureDiagram(String diagram, StructureDiagramEditor editor) throws Exception {
        Map<String, IPresentation> shown = new HashMap<>();
        List<Map<String, Object>> presentations = view(diagram);
        for (Map<String, Object> p : presentations) {
            if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)) {
                continue;
            }
            Map<String, Object> r = rect(diagram, p);
            Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
            String type = type(elements.get(element));
            INodePresentation node;
            if ("comment".equals(type)) {
                node = note(editor, element, at);
            } else if ("instance".equals(type) && editor instanceof ClassDiagramEditor cd) {
                node = (INodePresentation) attempt(element, () -> instance(cd, element, at));
            } else if (created.get(element) instanceof IElement model
                    && !(model instanceof IAttribute) && !(model instanceof IOperation)) {
                node = (INodePresentation) attempt(element, () -> editor.createNodePresentation(model, at));
            } else {
                continue;
            }
            if (node == null) {
                continue;
            }
            // Switching the notation resets the size, so the style goes first.
            style(node, p, type);
            // Instances get their place when they are created and cannot be moved afterwards.
            if (!"instance".equals(type)) {
                node.setLocation(at);
            }
            resize(node, num(r, "width"), num(r, "height"));
            shown.put((String) p.get("id"), node);
        }
        for (Map<String, Object> p : presentations) {
            if ("edge".equals(p.get("shape"))) {
                edge(editor, diagram, p, shown);
            }
        }
    }

    // ----- Activity diagrams ------------------------------------------------------------------

    /// Partitions first, since Astah lines up the lanes itself; then the other nodes, the flows
    /// and the note anchors. Nodes, flows and partitions of the activity that the diagram does
    /// not show are lost: Astah creates them only on a diagram.
    private void activityDiagram(String diagram, String activity, ActivityDiagramEditor editor) throws Exception {
        Map<String, IPresentation> shown = new HashMap<>();
        List<Map<String, Object>> presentations = view(diagram);
        partitions(diagram, activity, null, editor, presentations, shown);
        for (Map<String, Object> p : presentations) {
            if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)
                    || created.containsKey(element) && !"comment".equals(type(elements.get(element)))) {
                continue;
            }
            Map<String, Object> r = rect(diagram, p);
            Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
            Map<String, Object> e = elements.get(element);
            INodePresentation node;
            if ("comment".equals(type(e))) {
                node = note(editor, element, at);
            } else if ("activityNode".equals(type(e))) {
                node = (INodePresentation) attempt(element, () -> activityNode(editor, element, at));
            } else {
                continue;
            }
            if (node == null) {
                continue;
            }
            node.setLocation(at);
            resize(node, num(r, "width"), num(r, "height"));
            fill(node, p);
            shown.put((String) p.get("id"), node);
        }
        for (Map<String, Object> p : presentations) {
            if ("edge".equals(p.get("shape"))) {
                edge(editor, diagram, p, shown);
            }
        }
        for (String child : children(activity)) {
            String type = type(elements.get(child));
            if (!created.containsKey(child) && !"diagram".equals(type)) {
                drop(type);
            }
        }
    }

    /// The lanes owned by `owner`, left to right, each followed by its own lanes.
    private void partitions(String diagram, String owner, INodePresentation superLane, ActivityDiagramEditor editor,
            List<Map<String, Object>> presentations, Map<String, IPresentation> shown) throws Exception {
        List<Map<String, Object>> lanes = new ArrayList<>();
        for (Map<String, Object> p : presentations) {
            if (p.get("element") instanceof String element && "partition".equals(type(elements.get(element)))
                    && owner.equals(owners.get(element)) && !created.containsKey(element)) {
                lanes.add(p);
            }
        }
        lanes.sort((a, b) -> Double.compare(num(rect(diagram, a), "x"), num(rect(diagram, b), "x")));
        INodePresentation previous = null;
        for (Map<String, Object> p : lanes) {
            String element = (String) p.get("element");
            INodePresentation after = previous;
            // Astah lines the lane up after `after`, inside `superLane`.
            INodePresentation lane = (INodePresentation) attempt(element,
                () -> named(element, n -> editor.createPartition(superLane, after, n, false)));
            if (lane == null) {
                continue;
            }
            created.put(element, lane.getModel());
            common(element, lane.getModel(), Set.of());
            Map<String, Object> r = rect(diagram, p);
            resize(lane, num(r, "width"), num(r, "height"));
            fill(lane, p);
            shown.put((String) p.get("id"), lane);
            previous = lane;
            partitions(diagram, element, lane, editor, presentations, shown);
        }
        if (superLane == null && previous != null) {
            attempt(owner, () -> {
                placeLanes(diagram, lanes, shown);
                return null;
            });
        }
    }

    /// Astah puts top-level lanes below a "dimension" header of its own, at a place of its own,
    /// and only moves a lane within its header. So the header goes above UmlCraft's lanes first,
    /// as wide as they are; then the lanes follow.
    private void placeLanes(String diagram, List<Map<String, Object>> lanes, Map<String, IPresentation> shown)
            throws Exception {
        double left = Double.MAX_VALUE;
        double top = Double.MAX_VALUE;
        double right = -Double.MAX_VALUE;
        INodePresentation any = null;
        for (Map<String, Object> p : lanes) {
            if (shown.get(p.get("id")) instanceof INodePresentation lane) {
                Map<String, Object> r = rect(diagram, p);
                left = Math.min(left, num(r, "x"));
                top = Math.min(top, num(r, "y"));
                right = Math.max(right, num(r, "x") + num(r, "width"));
                any = lane;
            }
        }
        IPartition dimension = any == null ? null : ((IPartition) any.getModel()).getSuperPartition();
        if (dimension == null) {
            return;
        }
        for (IPresentation p : dimension.getPresentations()) {
            if (p instanceof INodePresentation header) {
                header.setLocation(new Point2D.Double(left, top - header.getHeight()));
                resize(header, right - left, header.getHeight());
            }
        }
        for (Map<String, Object> p : lanes) {
            if (shown.get(p.get("id")) instanceof INodePresentation lane) {
                Map<String, Object> r = rect(diagram, p);
                lane.setLocation(new Point2D.Double(num(r, "x"), num(r, "y")));
            }
        }
    }

    private INodePresentation activityNode(ActivityDiagramEditor editor, String id, Point2D at) throws Exception {
        Map<String, Object> e = elements.get(id);
        INodePresentation node = switch (text(e, "node")) {
            case "", "action" -> (INodePresentation) named(id, n -> editor.createAction(n, at));
            case "initial" -> editor.createInitialNode(controlName(e, "InitialNode"), at);
            case "activityFinal" -> editor.createFinalNode(controlName(e, "ActivityFinal"), at);
            case "flowFinal" -> editor.createFlowFinalNode(controlName(e, "FlowFinal"), at);
            case "decision" -> editor.createDecisionMergeNode(null, at);
            case "merge" -> {
                // The API only creates combined decision and merge nodes.
                drop("mergeNode");
                yield editor.createDecisionMergeNode(null, at);
            }
            case "fork" -> editor.createForkNode(null, at);
            case "join" -> editor.createJoinNode(null, at);
            case "object" -> {
                Object t = e.get("typeRef") == null ? null : type(e.get("typeRef"));
                IClass base = t instanceof IClass c ? c : null;
                if (t != null && base == null) {
                    drop("objectNodeType");
                }
                yield (INodePresentation) named(id, n -> editor.createObjectNode(n, base, at));
            }
            default -> throw new IllegalStateException(text(e, "node"));
        };
        if (!name(e).isEmpty() && !name(e).equals(((INamedElement) node.getModel()).getName())) {
            ((INamedElement) node.getModel()).setName(name(e));
        }
        created.put(id, node.getModel());
        common(id, (IElement) node.getModel(), Set.of());
        return node;
    }

    /// UmlCraft leaves control nodes unnamed, Astah wants a name: an unnamed one gets a numbered
    /// default name such as Astah gives (`InitialNode0`), which the import drops again.
    private String controlName(Map<String, Object> e, String stem) {
        if (!name(e).isEmpty()) {
            return name(e);
        }
        int n = controlNames.merge(stem, 1, Integer::sum) - 1;
        return stem + n;
    }

    // ----- State machine diagrams -------------------------------------------------------------

    /// States from the outside in, so every composite state exists, at its size, before its
    /// substates go into it; then transitions and note anchors. What the diagram does not show
    /// is lost, as on activity diagrams.
    private void stateDiagram(String diagram, String machine, StateMachineDiagramEditor editor) throws Exception {
        Map<String, IPresentation> shown = new HashMap<>();
        Map<String, INodePresentation> nodeOf = new HashMap<>();
        List<Map<String, Object>> presentations = new ArrayList<>(view(diagram));
        presentations.sort((a, b) -> Integer.compare(depth(a.get("element")), depth(b.get("element"))));
        for (Map<String, Object> p : presentations) {
            if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)) {
                continue;
            }
            Map<String, Object> r = rect(diagram, p);
            Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
            Map<String, Object> e = elements.get(element);
            INodePresentation node;
            if ("comment".equals(type(e))) {
                node = note(editor, element, at);
            } else if (Set.of("state", "finalState", "pseudostate").contains(type(e)) && !created.containsKey(element)) {
                INodePresentation parent = nodeOf.get(owners.get(element));
                node = (INodePresentation) attempt(element, () -> vertex(editor, element, parent, at, r));
            } else {
                continue;
            }
            if (node == null) {
                continue;
            }
            node.setLocation(at);
            // Astah keeps room for entry, do and exit, which makes a state at least 81 high;
            // a state without them does not need it.
            if ("state".equals(type(e)) && text(e, "entry").isEmpty() && text(e, "doActivity").isEmpty()
                    && text(e, "exit").isEmpty()) {
                node.setProperty("action_visibility", "false");
            }
            resize(node, num(r, "width"), num(r, "height"));
            fill(node, p);
            shown.put((String) p.get("id"), node);
            nodeOf.putIfAbsent(element, node);
        }
        for (Map<String, Object> p : view(diagram)) {
            if ("edge".equals(p.get("shape"))) {
                edge(editor, diagram, p, shown);
            }
        }
        for (String child : subtree(machine)) {
            String type = type(elements.get(child));
            if (!created.containsKey(child) && Set.of("state", "finalState", "pseudostate", "transition").contains(type)) {
                drop(type);
            }
        }
    }

    private INodePresentation vertex(StateMachineDiagramEditor editor, String id, INodePresentation parent, Point2D at,
            Map<String, Object> r) throws Exception {
        Map<String, Object> e = elements.get(id);
        INodePresentation node = switch (type(e)) {
            case "state" -> (INodePresentation) named(id, n -> editor.createState(n, parent, at));
            case "finalState" -> editor.createFinalState(parent, at);
            default -> switch (text(e, "pseudostate")) {
                case "", "initial" -> editor.createInitialPseudostate(parent, at);
                case "choice" -> editor.createChoicePseudostate(parent, at);
                case "junction" -> editor.createJunctionPseudostate(parent, at);
                case "fork" -> editor.createForkPseudostate(parent, at, num(r, "width"), num(r, "height"));
                case "join" -> editor.createJoinPseudostate(parent, at, num(r, "width"), num(r, "height"));
                case "shallowHistory" -> editor.createShallowHistoryPseudostate(parent, at);
                case "deepHistory" -> editor.createDeepHistoryPseudostate(parent, at);
                default -> throw new IllegalStateException(text(e, "pseudostate"));
            };
        };
        INamedElement model = (INamedElement) node.getModel();
        if (!name(e).isEmpty() && !name(e).equals(model.getName())) {
            model.setName(name(e));
        }
        if (model instanceof IState state && "state".equals(type(e))) {
            state.setEntry(text(e, "entry"));
            state.setDoActivity(text(e, "doActivity"));
            state.setExit(text(e, "exit"));
        }
        created.put(id, model);
        common(id, model, Set.of());
        return node;
    }

    /// A transition between two shown vertices, with its trigger, guard and effect.
    private ILinkPresentation transition(StateMachineDiagramEditor editor, String id, INodePresentation source,
            INodePresentation target) throws Exception {
        Map<String, Object> e = elements.get(id);
        ILinkPresentation link = editor.createTransition(source, target);
        ITransition transition = (ITransition) link.getModel();
        if (!text(e, "trigger").isEmpty()) {
            transition.setEvent(text(e, "trigger"));
        } else if (!transition.getEvent().isEmpty()) {
            // The API gives a transition leaving a state an event called "Trigger"; null removes
            // it, an empty text is refused.
            transition.setEvent(null);
        }
        if (!text(e, "guard").isEmpty()) {
            transition.setGuard(text(e, "guard"));
        }
        if (!text(e, "effect").isEmpty()) {
            transition.setAction(text(e, "effect"));
        }
        if (!name(e).isEmpty()) {
            transition.setName(name(e));
        }
        common(id, transition, Set.of());
        created.put(id, transition);
        return link;
    }

    /// How deep `id` lies in the ownership tree; presentations without an element come last.
    private int depth(Object id) {
        if (!(id instanceof String element)) {
            return Integer.MAX_VALUE;
        }
        int depth = 0;
        for (String o = owners.get(element); o != null; o = owners.get(o)) {
            depth++;
        }
        return depth;
    }

    private List<String> subtree(String id) {
        List<String> result = new ArrayList<>();
        for (String child : children(id)) {
            result.add(child);
            result.addAll(subtree(child));
        }
        return result;
    }

    /// A flow between two shown activity nodes, with its name and guard.
    private ILinkPresentation flow(ActivityDiagramEditor editor, String id, INodePresentation source,
            INodePresentation target) throws Exception {
        Map<String, Object> e = elements.get(id);
        ILinkPresentation link = editor.createFlow(source, target);
        IFlow flow = (IFlow) link.getModel();
        if (!name(e).isEmpty()) {
            flow.setName(name(e));
        }
        if (!text(e, "guard").isEmpty()) {
            flow.setGuard(text(e, "guard"));
        }
        common(id, flow, Set.of());
        created.put(id, flow);
        return link;
    }

    /// A fill color of the node's own; Astah's defaults stay otherwise.
    private static void fill(INodePresentation node, Map<String, Object> p) throws InvalidEditingException {
        Map<String, Object> style = (Map<String, Object>) p.get("style");
        if (style != null && style.get("fill") instanceof String f && f.length() >= 7) {
            node.setProperty("fill.color", f.substring(0, 7).toUpperCase());
        }
    }

    /// An instance specification of its classifier. The API draws every instance as an object, so
    /// component and node instances lose their kind.
    private INodePresentation instance(ClassDiagramEditor editor, String id, Point2D at) throws Exception {
        Map<String, Object> e = elements.get(id);
        INodePresentation node = editor.createInstanceSpecification(name(e), at);
        IInstanceSpecification instance = (IInstanceSpecification) node.getModel();
        if (e.get("classifier") instanceof String classifier && created.get(classifier) instanceof IClass c) {
            instance.setClassifier(c);
        }
        if (!text(e, "instance").isEmpty() && !"object".equals(text(e, "instance"))) {
            drop("instanceKind");
        }
        created.put(id, instance);
        common(id, instance, Set.of());
        return node;
    }

    private INodePresentation note(BasicDiagramEditor editor, String comment, Point2D at) throws Exception {
        shownComments.add(comment);
        INodePresentation note = editor.createNote(text(elements.get(comment), "body"), at);
        common(comment, note.getModel(), Set.of());
        return note;
    }

    private void edge(
        BasicDiagramEditor editor, String diagram, Map<String, Object> p, Map<String, IPresentation> shown
    ) {
        IPresentation source = shown.get(p.get("source"));
        IPresentation target = shown.get(p.get("target"));
        if (source == null || target == null) {
            return;
        }
        Object element = p.get("element");
        attempt(element == null ? (String) p.get("id") : (String) element, () -> {
            ILinkPresentation link;
            if (element == null) {
                // A note anchor runs from the note to what it annotates.
                if (isNote(source) && !isNote(target)) {
                    link = editor.createNoteAnchor((INodePresentation) source, target);
                } else if (isNote(target)) {
                    link = editor.createNoteAnchor((INodePresentation) target, source);
                } else {
                    return null;
                }
            } else if (editor instanceof ActivityDiagramEditor ad
                    && "activityEdge".equals(type(elements.get(element)))
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = flow(ad, (String) element, s, t);
            } else if (editor instanceof StateMachineDiagramEditor sm
                    && "transition".equals(type(elements.get(element)))
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = transition(sm, (String) element, s, t);
            } else if (editor instanceof StructureDiagramEditor sd && created.get(element) instanceof IElement model
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = sd.createLinkPresentation(model, s, t);
            } else {
                return null;
            }
            if (!route(link, source, target, p)) {
                drop("edgeRoute");
            }
            labels(link, diagram, p, element);
            return link;
        });
    }

    private static boolean isNote(IPresentation p) {
        return "Note".equals(p.getType());
    }

    /// Runs the line from the source anchor through the waypoints to the target anchor. Astah
    /// derives where the line meets a node from its first and last point, relative to the node.
    /// Returns false if Astah refused the line; it then keeps its own.
    private static boolean route(ILinkPresentation link, IPresentation source, IPresentation target,
            Map<String, Object> p) {
        List<Object> waypoints = list(p.get("waypoints"));
        Object sourceAnchor = p.get("sourceAnchor");
        Object targetAnchor = p.get("targetAnchor");
        if (waypoints.isEmpty() && sourceAnchor == null && targetAnchor == null) {
            return true;
        }
        if (!(source instanceof INodePresentation s) || !(target instanceof INodePresentation t)) {
            return true;
        }
        List<Point2D> points = new ArrayList<>();
        points.add(anchorPoint(s, sourceAnchor));
        for (Object w : waypoints) {
            Map<String, Object> point = (Map<String, Object>) w;
            points.add(new Point2D.Double(num(point, "x"), num(point, "y")));
        }
        points.add(anchorPoint(t, targetAnchor));
        // Astah may store a link the other way round, from supplier to client or from general to
        // specific, and wants the points in its order. Its current first point tells which end
        // comes first; getSource() does not always (generalizations).
        Point2D first = link.getAllPoints()[0];
        if (s != t && t.getRectangle().contains(first) && !s.getRectangle().contains(first)) {
            Collections.reverse(points);
        }
        try {
            link.setAllPoints(points.toArray(new Point2D[0]));
            return true;
        } catch (InvalidEditingException e) {
            return false;
        }
    }

    /// A point inside the node, given relative to its bounds; the center without an anchor.
    private static Point2D anchorPoint(INodePresentation node, Object anchor) {
        Map<String, Object> a = anchor instanceof Map<?, ?> m ? (Map<String, Object>) m : Map.of("x", 0.5, "y", 0.5);
        Point2D at = node.getLocation();
        return new Point2D.Double(at.getX() + num(a, "x") * node.getWidth(), at.getY() + num(a, "y") * node.getHeight());
    }

    /// Labels placed by hand. Astah keeps them per association end, which need not be the end
    /// the line starts at.
    private void labels(ILinkPresentation link, String diagram, Map<String, Object> p, Object element)
            throws InvalidEditingException {
        Map<String, Object> l = (Map<String, Object>) layouts.get(diagram);
        if (l == null || !(l.get("labels") instanceof Map<?, ?> all)
                || !(all.get(p.get("id")) instanceof Map<?, ?> found)) {
            return;
        }
        Map<String, Object> placed = (Map<String, Object>) found;
        boolean swapped = swappedEnds.contains(element);
        String source = swapped ? "end_b" : "end_a";
        String target = swapped ? "end_a" : "end_b";
        label(link, "name", placed.get("name"));
        label(link, source + ".name", placed.get("sourceRole"));
        label(link, source + ".multiplicity", placed.get("sourceMultiplicity"));
        label(link, target + ".name", placed.get("targetRole"));
        label(link, target + ".multiplicity", placed.get("targetMultiplicity"));
    }

    private static void label(ILinkPresentation link, String key, Object point) throws InvalidEditingException {
        if (point instanceof Map<?, ?> m) {
            Map<String, Object> at = (Map<String, Object>) m;
            link.setProperty(key + ".point.x", Double.toString(num(at, "x")));
            link.setProperty(key + ".point.y", Double.toString(num(at, "y")));
        }
    }

    private static void resize(INodePresentation node, double width, double height) {
        try {
            // Keep UmlCraft's size instead of letting Astah fit the content again.
            if (node.getProperties().containsKey("auto_resize")) {
                node.setProperty("auto_resize", "false");
            }
            node.setWidth(width);
            node.setHeight(height);
        } catch (InvalidEditingException e) {
            // Some figures (actors drawn as stick figures, for example) have a fixed size.
        }
    }

    private void style(INodePresentation node, Map<String, Object> p, String type) throws InvalidEditingException {
        Map<String, Object> style = (Map<String, Object>) p.get("style");
        if ("interface".equals(type)) {
            boolean iconic = style != null && Boolean.TRUE.equals(style.get("iconic"));
            node.setProperty("notation_type", iconic ? "icon" : "normal");
        }
        // Without a color of its own a node gets Astah's usual default, not the API's white.
        String fill = style != null && style.get("fill") instanceof String f && f.length() >= 7
            ? f.substring(0, 7).toUpperCase() : DEFAULT_FILL;
        node.setProperty("fill.color", fill);
        if (style == null) {
            return;
        }
        if (Boolean.TRUE.equals(style.get("hideAttributes"))) {
            node.setProperty("attribute_compartment_visibility", "false");
        }
        if (Boolean.TRUE.equals(style.get("hideOperations"))) {
            node.setProperty("operation_compartment_visibility", "false");
        }
    }

    // ----- Sequence diagrams ------------------------------------------------------------------

    /// A combined fragment shown on the diagram, with the messages inside each operand.
    private record Fragment(String id, Map<String, Object> frame, Map<String, Object> shape, List<Operand> operands) {
    }

    private record Operand(String id, String guard, List<IPresentation> messages) {
    }

    /// An activation bar as UmlCraft stores it, and the bar Astah made for it.
    private static final class Bar {
        final String lane;
        final double top;
        final double bottom;
        /// Its fill; the API's bars are white, Astah's editor makes them yellow.
        final String fill;
        INodePresentation astah;

        Bar(String lane, double top, double bottom, String fill) {
            this.lane = lane;
            this.top = top;
            this.bottom = bottom;
            this.fill = fill;
        }
    }

    /// Builds a sequence diagram from the geometry UmlCraft stores, which is Astah's: heads,
    /// lifeline lengths, the heights of messages, bars and frames. Astah puts the heads at 30,
    /// so everything moves up or down by as much; x stays.
    private final class Sequence {
        private final String diagram;
        private final SequenceDiagramEditor editor;
        /// UmlCraft lifeline presentation → Astah lifeline.
        private final Map<String, INodePresentation> lanes = new HashMap<>();
        private final Map<String, Map<String, Object>> laneShapes = new HashMap<>();
        /// Message element → its presentation in UmlCraft.
        private final Map<String, Map<String, Object>> messageOf = new HashMap<>();
        private final Map<String, Map<String, Object>> nodeOf = new HashMap<>();
        private final List<Bar> bars = new ArrayList<>();
        /// Message element → the Astah message made for it.
        private final Map<String, ILinkPresentation> made = new HashMap<>();
        private final List<Fragment> fragments = new ArrayList<>();
        private double dy;

        Sequence(String diagram, SequenceDiagramEditor editor) {
            this.diagram = diagram;
            this.editor = editor;
        }

        private double y(double umlcraft) {
            return umlcraft + dy;
        }

        void build() throws Exception {
            List<Map<String, Object>> presentations = view(diagram);
            List<Map<String, Object>> lifelines = new ArrayList<>();
            for (Map<String, Object> p : presentations) {
                String shape = (String) p.get("shape");
                if ("lifeline".equals(shape)) {
                    lifelines.add(p);
                } else if ("activation".equals(shape)) {
                    double top = num(p, "top");
                    Map<String, Object> style = (Map<String, Object>) p.get("style");
                    String fill = style != null && style.get("fill") instanceof String f && f.length() >= 7
                        ? f.substring(0, 7).toUpperCase() : DEFAULT_FILL;
                    bars.add(new Bar((String) p.get("lifeline"), top, top + num(p, "height"), fill));
                } else if ("message".equals(shape) && p.get("element") instanceof String element) {
                    messageOf.putIfAbsent(element, p);
                } else if (p.get("element") instanceof String element) {
                    nodeOf.putIfAbsent(element, p);
                }
            }
            lifelines.sort((a, b) -> Double.compare(num(head(a), "x"), num(head(b), "x")));
            double top = Double.MAX_VALUE;
            for (Map<String, Object> p : lifelines) {
                top = Math.min(top, num(head(p), "y"));
            }
            dy = lifelines.isEmpty() ? 0 : 30 - top;

            for (Map<String, Object> p : lifelines) {
                String element = (String) p.get("element");
                Map<String, Object> head = head(p);
                INodePresentation lane = editor.createLifeline(name(elements.get(element)), num(head, "x"));
                lanes.put((String) p.get("id"), lane);
                laneShapes.put((String) p.get("id"), p);
                ILifeline model = (ILifeline) lane.getModel();
                if (created.get(elements.get(element).get("represents")) instanceof IClass base) {
                    model.setBase(base);
                }
                common(element, model, Set.of());
                created.putIfAbsent(element, model);
                resize(lane, num(head, "width"), num(head, "height"));
            }

            walk(owners.get(diagram), null);
            for (Fragment f : fragments) {
                attempt(f.id(), () -> fragment(f));
            }
            displayAll();

            // Astah grows bars itself, mostly as UmlCraft does; what it can be told is kept.
            for (Bar bar : bars) {
                if (bar.astah != null) {
                    try {
                        bar.astah.setHeight(bar.bottom - bar.top);
                    } catch (InvalidEditingException e) {
                        drop("barHeight");
                    }
                }
            }
            for (Map.Entry<String, INodePresentation> lane : lanes.entrySet()) {
                double length = num(laneShapes.get(lane.getKey()), "length");
                try {
                    lane.getValue().setProperty("lifeline_length", Double.toString(length));
                } catch (InvalidEditingException e) {
                    // Astah keeps lifelines long enough for what lies on them.
                    drop("lifelineLength");
                }
            }
            for (Map<String, Object> p : presentations) {
                if (p.get("element") instanceof String element && "comment".equals(type(elements.get(element)))
                        && "node".equals(p.get("shape"))) {
                    Map<String, Object> r = rect(diagram, p);
                    attempt(element, () -> {
                        INodePresentation note = note(editor, element, new Point2D.Double(num(r, "x"), y(num(r, "y"))));
                        resize(note, num(r, "width"), num(r, "height"));
                        return note;
                    });
                }
            }
        }

        private Map<String, Object> head(Map<String, Object> lifeline) {
            return (Map<String, Object>) lifeline.get("head");
        }

        /// Creates the messages of an interaction or operand from top to bottom and collects the
        /// fragments, outer ones first. Returns the message presentations created, nested ones
        /// included.
        private List<IPresentation> walk(String container, List<IPresentation> into) {
            List<IPresentation> made = into == null ? new ArrayList<>() : into;
            for (String child : children(container)) {
                String type = type(elements.get(child));
                if ("message".equals(type)) {
                    Object link = attempt(child, () -> message(child));
                    if (link instanceof IPresentation p) {
                        made.add(p);
                    }
                } else if ("interactionUse".equals(type)) {
                    Object use = attempt(child, () -> interactionUse(child));
                    if (use instanceof IPresentation p) {
                        made.add(p);
                    }
                } else if ("combinedFragment".equals(type)) {
                    Map<String, Object> node = nodeOf.get(child);
                    List<Operand> operands = new ArrayList<>();
                    Fragment fragment = node == null ? null : new Fragment(child, rect(diagram, node), node, operands);
                    if (fragment != null) {
                        fragments.add(fragment);
                    }
                    for (String operand : children(child)) {
                        if (!"interactionOperand".equals(type(elements.get(operand)))) {
                            continue;
                        }
                        List<IPresentation> inside = walk(operand, new ArrayList<>());
                        operands.add(new Operand(operand, text(elements.get(operand), "guard"), inside));
                        made.addAll(inside);
                    }
                }
            }
            return made;
        }

        /// The innermost bar on `lane` at `y`.
        private Bar barAt(String lane, double y) {
            Bar best = null;
            for (Bar bar : bars) {
                if (bar.lane.equals(lane) && bar.top - 0.5 <= y && y <= bar.bottom + 0.5
                        && (best == null || bar.bottom - bar.top < best.bottom - best.top)) {
                    best = bar;
                }
            }
            return best;
        }

        private Bar barStarting(String lane, double y) {
            for (Bar bar : bars) {
                if (bar.lane.equals(lane) && Math.abs(bar.top - y) < 0.5) {
                    return bar;
                }
            }
            return null;
        }

        /// The message whose bar a reply from `lane` at `y` ends.
        private String callEndedAt(String lane, double y) {
            Bar bar = barAt(lane, y);
            if (bar == null) {
                return null;
            }
            for (Map.Entry<String, Map<String, Object>> m : messageOf.entrySet()) {
                Map<String, Object> p = m.getValue();
                double end = p.get("endY") instanceof Number n ? n.doubleValue() : num(p, "y");
                if (lane.equals(p.get("target")) && Math.abs(end - bar.top) < 0.5
                        && !"reply".equals(elements.get(m.getKey()).get("sort"))) {
                    return m.getKey();
                }
            }
            return null;
        }

        /// Remembers the Astah bar at an end of `link` for the UmlCraft bar starting there.
        private void keep(IPresentation end, String lane, double y) {
            if (end instanceof INodePresentation n && "Activation".equals(n.getType())) {
                Bar bar = barStarting(lane, y);
                if (bar != null && bar.astah == null) {
                    bar.astah = n;
                    try {
                        n.setProperty("fill.color", bar.fill);
                    } catch (InvalidEditingException e) {
                        drop("barColor");
                    }
                }
            }
        }

        /// Display options of a message: arguments shown or not, and its label where UmlCraft has
        /// it. The label is kept relative to the sender's lifeline. Astah moves a label by as much
        /// as `name.point` differs from its own value; where Astah puts it by itself comes with
        /// the layout.
        private void display(ILinkPresentation link, Map<String, Object> p, String from, double at)
                throws InvalidEditingException {
            Map<String, Object> style = (Map<String, Object>) p.get("style");
            if (style != null && Boolean.TRUE.equals(style.get("hideParameters"))) {
                link.setProperty("parameter_visibility", "false");
            }
            Map<String, Object> labels = (Map<String, Object>) p.get("labels");
            if (labels != null && labels.get("name") instanceof Map<?, ?> offset) {
                Map<String, Object> head = head(laneShapes.get(from));
                double center = num(head, "x") + num(head, "width") / 2;
                double x = center + num((Map<String, Object>) offset, "x");
                double y = y(at + num((Map<String, Object>) offset, "y"));
                Object defaults = layouts.get(diagram) instanceof Map<?, ?> l ? l.get("labelDefaults") : null;
                Object own = defaults instanceof Map<?, ?> d ? d.get(p.get("id")) : null;
                double point = Double.parseDouble(link.getProperty("name.point.x"));
                double shift = own instanceof Number n ? point - n.doubleValue() : -2.45;
                link.setProperty("name.point.x", Double.toString(x + shift));
                link.setProperty("name.point.y", Double.toString(y));
            }
        }

        private Object message(String id) throws Exception {
            Map<String, Object> e = elements.get(id);
            Map<String, Object> p = messageOf.get(id);
            String from = p == null ? null : (String) p.get("source");
            String to = p == null ? null : (String) p.get("target");
            if (p == null || !lanes.containsKey(from) || !lanes.containsKey(to)) {
                drop("message");
                return null;
            }
            String sort = e.get("sort") == null ? "synchronous" : (String) e.get("sort");
            double at = num(p, "y");
            double end = p.get("endY") instanceof Number n ? n.doubleValue() : at;
            String name = name(e);
            String gate = text(e, "gate");
            if ("found".equals(gate) || "lost".equals(gate)) {
                return gateMessage(id, p, gate, sort, to, at);
            }
            Bar bar = barAt(from, at);
            INodePresentation source = bar != null && bar.astah != null ? bar.astah : lanes.get(from);
            INodePresentation target = lanes.get(to);
            ILinkPresentation link;
            // Astah keeps a reply's return value on the call it answers.
            IMessage answered = null;
            switch (sort) {
                case "create" -> link = editor.createCreateMessage(name, source, target, y(at));
                case "destroy" -> link = editor.createDestroyMessage(name, source, target, y(at));
                case "reply" -> {
                    String call = callEndedAt(from, at);
                    if (call != null && made.get(call) != null) {
                        // Astah puts a reply at the end of the call's bar, as UmlCraft does.
                        answered = (IMessage) made.get(call).getModel();
                        link = editor.createReturnMessage(name, made.get(call));
                    } else {
                        drop("unmatchedReply");
                        link = editor.createMessage(name, source, target, y(at));
                    }
                }
                default -> {
                    link = editor.createMessage(name, source, target, y(at));
                    if ("asynchronous".equals(sort)) {
                        ((IMessage) link.getModel()).setAsynchronous(true);
                    }
                }
            }
            keep(link.getSource(), from, at);
            keep(link.getTarget(), to, end);
            made.put(id, link);
            IMessage message = (IMessage) link.getModel();
            if (!text(e, "arguments").isEmpty()) {
                if (message.isReturnMessage()) {
                    drop("replyArguments");
                } else {
                    message.setArgument(text(e, "arguments"));
                }
            }
            if (!text(e, "guard").isEmpty()) {
                message.setGuard(text(e, "guard"));
            }
            if (!text(e, "returnValue").isEmpty()) {
                if (answered != null) {
                    answered.setReturnValue(text(e, "returnValue"));
                } else if (!message.isReturnMessage()) {
                    message.setReturnValue(text(e, "returnValue"));
                }
            }
            if (e.get("operation") != null) {
                if (created.get(e.get("operation")) instanceof IOperation op) {
                    try {
                        message.setOperation(op);
                    } catch (InvalidEditingException x) {
                        drop("messageOperation");
                    }
                } else {
                    drop("messageOperation");
                }
            }
            common(id, message, Set.of());
            return link;
        }

        /// Sets the display options of every message made, once their texts are complete:
        /// Astah centers a label anew when its text changes.
        private void displayAll() throws InvalidEditingException {
            for (Map.Entry<String, ILinkPresentation> m : made.entrySet()) {
                Map<String, Object> p = messageOf.get(m.getKey());
                display(m.getValue(), p, (String) p.get("source"), num(p, "y"));
            }
        }

        /// A "ref" frame across the lifelines it covers (all of them when none are given) where
        /// it is drawn. Its diagram is set once all diagrams exist.
        private INodePresentation interactionUse(String id) throws Exception {
            Map<String, Object> e = elements.get(id);
            Map<String, Object> node = nodeOf.get(id);
            if (node == null) {
                drop("interactionUse");
                return null;
            }
            List<ILifeline> covered = new ArrayList<>();
            for (Object lifeline : list(e.get("covered"))) {
                if (created.get(lifeline) instanceof ILifeline l) {
                    covered.add(l);
                }
            }
            if (covered.isEmpty()) {
                for (INodePresentation lane : lanes.values()) {
                    covered.add((ILifeline) lane.getModel());
                }
            }
            Map<String, Object> r = rect(diagram, node);
            // Astah wants a name; an unnamed frame shows the interaction it refers to, as in
            // UmlCraft.
            String name = name(e);
            if (name.isEmpty() && e.get("refersTo") instanceof String target) {
                name = name(elements.get(target));
            }
            if (name.isEmpty()) {
                name = "ref";
            }
            INodePresentation frame = editor.createInteractionUse(
                name, text(e, "arguments"), null, covered.toArray(new ILifeline[0]), y(num(r, "y")));
            place(frame, r);
            IInteractionUse use = (IInteractionUse) frame.getModel();
            if (e.get("refersTo") instanceof String target) {
                references.put(use, target);
            }
            common(id, use, Set.of());
            return frame;
        }

        /// Moves and sizes a frame as UmlCraft has it, as far as Astah lets it.
        private void place(INodePresentation frame, Map<String, Object> r) {
            try {
                frame.setLocation(new Point2D.Double(num(r, "x"), y(num(r, "y"))));
            } catch (InvalidEditingException e) {
                drop("framePosition");
            }
            resize(frame, num(r, "width"), num(r, "height"));
        }

        /// A message from a gate left of `lane` or to a gate right of it.
        private ILinkPresentation gateMessage(String id, Map<String, Object> p, String gate, String sort, String lane,
                double at) throws Exception {
            Map<String, Object> e = elements.get(id);
            INodePresentation lifeline = lanes.get(lane);
            double x = lifeline.getLocation().getX() + lifeline.getWidth() / 2;
            ILinkPresentation link;
            if ("found".equals(gate)) {
                double gx = p.get("gateX") instanceof Number n ? n.doubleValue() : x - 120;
                link = editor.createFoundMessage(name(e), new Point2D.Double(gx, y(at)), lifeline);
                keep(link.getTarget(), lane, at);
            } else {
                double gx = p.get("gateX") instanceof Number n ? n.doubleValue() : x + 120;
                Bar bar = barAt(lane, at);
                INodePresentation source = bar != null && bar.astah != null ? bar.astah : lifeline;
                link = editor.createLostMessage(name(e), source, new Point2D.Double(gx, y(at)));
                keep(link.getSource(), lane, at);
            }
            made.put(id, link);
            IMessage message = (IMessage) link.getModel();
            if ("asynchronous".equals(sort)) {
                message.setAsynchronous(true);
            }
            if (!text(e, "arguments").isEmpty()) {
                message.setArgument(text(e, "arguments"));
            }
            common(id, message, Set.of());
            return link;
        }

        /// Draws the fragment where UmlCraft has it; Astah takes the messages inside into its
        /// first operand. Further operands get their messages (an empty one cannot be added), the
        /// separators their places through `operand.N.length` and guards moved by hand theirs
        /// through `guard.N.point`.
        private Object fragment(Fragment f) throws Exception {
            Map<String, Object> e = elements.get(f.id());
            String operator = text(e, "operator").isEmpty() ? "alt" : text(e, "operator");
            Map<String, Object> r = f.frame();
            double top = num(r, "y");
            INodePresentation node = editor.createCombinedFragment(name(e), operator,
                new Point2D.Double(num(r, "x"), y(top)), num(r, "width"), num(r, "height"));
            List<Operand> made = new ArrayList<>();
            List<Integer> kept = new ArrayList<>();
            for (int i = 0; i < f.operands().size(); i++) {
                Operand o = f.operands().get(i);
                if (i == 0) {
                    made.add(o);
                    kept.add(i);
                } else if (o.messages().isEmpty()) {
                    drop("emptyOperand");
                } else {
                    try {
                        editor.addInteractionOperand(node, o.messages().toArray(new IPresentation[0]),
                            name(elements.get(o.id())), o.guard());
                    } catch (InvalidEditingException x) {
                        // Astah allows one operand for opt, loop, break and the like; the
                        // messages stay in the first one.
                        drop("extraOperand");
                        continue;
                    }
                    made.add(o);
                    kept.add(i);
                }
            }
            ICombinedFragment model = (ICombinedFragment) node.getModel();
            if (!name(e).isEmpty()) {
                model.setName(name(e));
            }
            IInteractionOperand[] operands = model.getInteractionOperands();
            for (int i = 0; i < operands.length && i < made.size(); i++) {
                operands[i].setGuard(made.get(i).guard());
                common(made.get(i).id(), operands[i], Set.of());
            }
            // Separators: each operand as long as from its separator to the next one.
            List<Object> separators = list(f.shape().get("separators"));
            List<Object> guards = list(f.shape().get("guards"));
            double previous = 0;
            for (int k = 0; k + 1 < made.size(); k++) {
                int next = kept.get(k + 1);
                if (next - 1 < separators.size() && separators.get(next - 1) instanceof Number s) {
                    node.setProperty("operand." + (k + 1) + ".length", Double.toString(s.doubleValue() - previous));
                    previous = s.doubleValue();
                }
            }
            for (int k = 0; k < made.size(); k++) {
                int i = kept.get(k);
                if (i < guards.size() && guards.get(i) instanceof Map<?, ?> moved) {
                    double operandTop = i == 0 ? top + 25.09375
                        : top + ((Number) separators.get(i - 1)).doubleValue();
                    double x = num(r, "x") + 10 + num((Map<String, Object>) moved, "x");
                    double gy = y(operandTop + 2 + num((Map<String, Object>) moved, "y"));
                    node.setProperty("guard." + (k + 1) + ".point.x", Double.toString(x));
                    node.setProperty("guard." + (k + 1) + ".point.y", Double.toString(gy));
                }
            }
            common(f.id(), model, Set.of());
            return node;
        }
    }

    // ----- Shared helpers ---------------------------------------------------------------------

    @FunctionalInterface
    private interface Creator {
        Object create(String name) throws Exception;
    }

    @FunctionalInterface
    private interface Step {
        Object run() throws Exception;
    }

    /// Runs one step of the export. A failure drops just this element and is reported.
    private Object attempt(String id, Step step) {
        try {
            return step.run();
        } catch (Exception e) {
            Map<String, Object> element = elements.get(id);
            String what = element == null ? "presentation" : type(element);
            drop(what);
            if (problems.size() < 20) {
                String name = element == null ? "" : " \"" + name(element) + "\"";
                problems.add(what + name + ": " + describe(e));
            }
            return null;
        }
    }

    /// Creates an element under its UmlCraft name. Astah rejects empty names and duplicates
    /// among siblings, so those get a numbered name instead.
    private Object named(String id, Creator creator) throws Exception {
        String base = name(elements.get(id));
        String name = base;
        for (int attempt = 2; ; attempt++) {
            try {
                Object made = creator.create(name);
                if (!name.equals(base)) {
                    drop("renamed");
                }
                return made;
            } catch (InvalidEditingException e) {
                boolean retry = InvalidEditingException.NAME_DOUBLE_ERROR_KEY.equals(e.getKey())
                    || InvalidEditingException.NO_NAME_ERROR_KEY.equals(e.getKey());
                if (!retry || attempt > 100) {
                    throw e;
                }
                String stem = base.isEmpty() ? type(elements.get(id)) : base;
                name = stem + " " + attempt;
            }
        }
    }

    /// Visibility, stereotypes, tagged values and documentation.
    private void common(String id, IElement target, Set<String> implied) {
        Map<String, Object> e = elements.get(id);
        if (e == null || target == null) {
            return;
        }
        try {
            if (target instanceof INamedElement n && e.get("visibility") instanceof String v) {
                n.setVisibility(v);
            }
            for (Object s : list(e.get("stereotypes"))) {
                if (!implied.contains(s) && !target.hasStereotype((String) s)) {
                    target.addStereotype((String) s);
                }
            }
            for (Object tv : list(e.get("taggedValues"))) {
                Map<String, Object> tag = (Map<String, Object>) tv;
                basic.createTaggedValue(target, text(tag, "key"), text(tag, "value"));
            }
            if (target instanceof INamedElement n && !text(e, "documentation").isEmpty()) {
                n.setDefinition(text(e, "documentation"));
            }
        } catch (InvalidEditingException x) {
            drop("details");
            if (problems.size() < 20) {
                problems.add(type(e) + " \"" + name(e) + "\": " + describe(x));
            }
        }
    }

    private void drop(String key) {
        dropped.merge(key, 1, Integer::sum);
    }

    private List<String> children(String id) {
        List<String> result = new ArrayList<>();
        Map<String, Object> e = elements.get(id);
        if (e != null) {
            for (Object c : list(e.get("children"))) {
                result.add((String) c);
            }
        }
        return result;
    }

    private List<Map<String, Object>> view(String diagram) {
        List<Map<String, Object>> v = views.get(diagram);
        return v == null ? List.of() : v;
    }

    /// A node's bounds as UmlCraft draws them, or as stored if the layout does not know it.
    private Map<String, Object> rect(String diagram, Map<String, Object> presentation) {
        Map<String, Object> l = (Map<String, Object>) layouts.get(diagram);
        if (l != null && l.get("nodes") instanceof Map<?, ?> nodes
                && nodes.get(presentation.get("id")) instanceof Map<?, ?> r) {
            return (Map<String, Object>) r;
        }
        return (Map<String, Object>) presentation.get("bounds");
    }

    private static List<Object> list(Object value) {
        return value instanceof List<?> l ? (List<Object>) l : List.of();
    }

    private static String type(Map<String, Object> element) {
        return element == null ? "" : text(element, "type");
    }

    private static String name(Map<String, Object> element) {
        return element == null ? "" : text(element, "name");
    }

    private static String text(Map<String, Object> map, String key) {
        return map.get(key) instanceof String s ? s : "";
    }

    private static boolean flag(Map<String, Object> map, String key) {
        return Boolean.TRUE.equals(map.get(key));
    }

    private static double num(Map<String, Object> map, String key) {
        return map.get(key) instanceof Number n ? n.doubleValue() : 0.0;
    }

    /// A small JSON reader: objects become insertion-ordered maps, numbers doubles.
    private static final class Json {
        private final String s;
        private int i;

        Json(String s) {
            this.s = s;
        }

        Object document() {
            Object value = value();
            skip();
            if (i != s.length()) {
                throw error("trailing characters");
            }
            return value;
        }

        private Object value() {
            skip();
            if (i >= s.length()) {
                throw error("unexpected end");
            }
            char c = s.charAt(i);
            switch (c) {
                case '{' -> {
                    i++;
                    Map<String, Object> map = new LinkedHashMap<>();
                    skip();
                    if (peek('}')) {
                        return map;
                    }
                    do {
                        skip();
                        String key = string();
                        skip();
                        expect(':');
                        map.put(key, value());
                        skip();
                    } while (peek(','));
                    expect('}');
                    return map;
                }
                case '[' -> {
                    i++;
                    List<Object> list = new ArrayList<>();
                    skip();
                    if (peek(']')) {
                        return list;
                    }
                    do {
                        list.add(value());
                        skip();
                    } while (peek(','));
                    expect(']');
                    return list;
                }
                case '"' -> {
                    return string();
                }
                default -> {
                    if (s.startsWith("true", i)) {
                        i += 4;
                        return Boolean.TRUE;
                    }
                    if (s.startsWith("false", i)) {
                        i += 5;
                        return Boolean.FALSE;
                    }
                    if (s.startsWith("null", i)) {
                        i += 4;
                        return null;
                    }
                    int start = i;
                    while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) {
                        i++;
                    }
                    if (start == i) {
                        throw error("unexpected character");
                    }
                    return Double.parseDouble(s.substring(start, i));
                }
            }
        }

        private String string() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (true) {
                if (i >= s.length()) {
                    throw error("unterminated string");
                }
                char c = s.charAt(i++);
                if (c == '"') {
                    return out.toString();
                }
                if (c != '\\') {
                    out.append(c);
                    continue;
                }
                char escape = s.charAt(i++);
                switch (escape) {
                    case 'n' -> out.append('\n');
                    case 't' -> out.append('\t');
                    case 'r' -> out.append('\r');
                    case 'b' -> out.append('\b');
                    case 'f' -> out.append('\f');
                    case 'u' -> {
                        out.append((char) Integer.parseInt(s.substring(i, i + 4), 16));
                        i += 4;
                    }
                    default -> out.append(escape);
                }
            }
        }

        private void skip() {
            while (i < s.length() && Character.isWhitespace(s.charAt(i))) {
                i++;
            }
        }

        private boolean peek(char c) {
            if (i < s.length() && s.charAt(i) == c) {
                i++;
                return true;
            }
            return false;
        }

        private void expect(char c) {
            if (!peek(c)) {
                throw error("expected '" + c + "'");
            }
        }

        private IllegalArgumentException error(String what) {
            return new IllegalArgumentException("malformed JSON at offset " + i + ": " + what);
        }
    }
}
