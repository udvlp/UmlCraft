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
import com.change_vision.jude.api.inf.editor.CompositeStructureDiagramEditor;
import com.change_vision.jude.api.inf.editor.CompositeStructureModelEditor;
import com.change_vision.jude.api.inf.editor.DiagramEditor;
import com.change_vision.jude.api.inf.editor.SequenceDiagramEditor;
import com.change_vision.jude.api.inf.editor.StateMachineDiagramEditor;
import com.change_vision.jude.api.inf.editor.StructureDiagramEditor;
import com.change_vision.jude.api.inf.editor.TransactionManager;
import com.change_vision.jude.api.inf.editor.UseCaseDiagramEditor;
import com.change_vision.jude.api.inf.editor.UseCaseModelEditor;
import com.change_vision.jude.api.inf.exception.InvalidEditingException;
import com.change_vision.jude.api.inf.model.IAction;
import com.change_vision.jude.api.inf.model.IActivity;
import com.change_vision.jude.api.inf.model.IActivityDiagram;
import com.change_vision.jude.api.inf.model.IAssociation;
import com.change_vision.jude.api.inf.model.IAssociationClass;
import com.change_vision.jude.api.inf.model.IAttribute;
import com.change_vision.jude.api.inf.model.IClass;
import com.change_vision.jude.api.inf.model.IClassifierTemplateParameter;
import com.change_vision.jude.api.inf.model.IConnector;
import com.change_vision.jude.api.inf.model.IPort;
import com.change_vision.jude.api.inf.model.ITemplateBinding;
import com.change_vision.jude.api.inf.model.ICombinedFragment;
import com.change_vision.jude.api.inf.model.IComponent;
import com.change_vision.jude.api.inf.model.IDependency;
import com.change_vision.jude.api.inf.model.IDiagram;
import com.change_vision.jude.api.inf.model.IElement;
import com.change_vision.jude.api.inf.model.IEnumeration;
import com.change_vision.jude.api.inf.model.IFlow;
import com.change_vision.jude.api.inf.model.IInstanceSpecification;
import com.change_vision.jude.api.inf.model.ILink;
import com.change_vision.jude.api.inf.model.ILinkEnd;
import com.change_vision.jude.api.inf.model.ISlot;
import com.change_vision.jude.api.inf.model.IInteractionOperand;
import com.change_vision.jude.api.inf.model.IInteractionUse;
import com.change_vision.jude.api.inf.model.ILifeline;
import com.change_vision.jude.api.inf.model.IMessage;
import com.change_vision.jude.api.inf.model.IModel;
import com.change_vision.jude.api.inf.model.INamedElement;
import com.change_vision.jude.api.inf.model.IObjectNode;
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
    /// Measured in Astah: where a message label stands left of the point the API gives it.
    private static final double LABEL_SHIFT = -2.45;
    /// How far a found or lost message without a gate position starts beside its lifeline.
    private static final double GATE_DISTANCE = 120;
    /// The top of a combined fragment's first operand below the frame's top: the tab's height.
    private static final double FIRST_OPERAND_TOP = 25.09375;
    /// Where a boundary's name stands below its top.
    private static final double BOUNDARY_NAME_TOP = 7;

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

    /// Reports a failure that is not one element's, for the first 20.
    private void problem(String what, Exception e) {
        if (problems.size() < 20) {
            problems.add(oneLine(what + ": " + describe(e)));
        }
    }

    /// A report line holds one finding: names with line breaks or tabs must not split it.
    private static String oneLine(String text) {
        return text.replaceAll("\\s+", " ");
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
    private CompositeStructureModelEditor structures;
    /// UmlCraft element id → Astah model element.
    private final Map<String, Object> created = new HashMap<>();
    private final List<String> templateParameters = new ArrayList<>();
    /// Bound classes and their bindings.
    private final Map<String, ITemplateBinding> bindings = new HashMap<>();
    private final List<String> features = new ArrayList<>();
    private final List<String> relationships = new ArrayList<>();
    /// The class of each association class → its association. Astah has one element for both,
    /// created once the classes at its ends exist.
    private final Map<String, String> associationClasses = new LinkedHashMap<>();
    private final List<String> diagrams = new ArrayList<>();
    private final Set<String> shownComments = new HashSet<>();
    private final Set<String> shownBoundaries = new HashSet<>();
    /// Default names handed out to unnamed control nodes, by stem.
    private final Map<String, Integer> controlNames = new HashMap<>();
    /// Vertices in orthogonal states and where they belong, placed again after the transaction.
    private final Map<INodePresentation, Map<String, Object>> replaced = new LinkedHashMap<>();
    /// UmlCraft interaction → the Astah sequence diagram that brought it along.
    private final Map<String, ISequenceDiagram> sequenceDiagrams = new HashMap<>();
    /// Interaction uses and the UmlCraft interaction they refer to, set once all diagrams exist.
    private final Map<IInteractionUse, String> references = new LinkedHashMap<>();
    /// UmlCraft activity → the Astah activity its diagram brought along.
    private final Map<String, IActivity> activities = new HashMap<>();
    /// Call behavior actions and the UmlCraft activity they call, set once all diagrams exist.
    private final Map<IAction, String> calls = new LinkedHashMap<>();
    /// UmlCraft state machine → the Astah diagram that brought it along. Submachine states
    /// refer to it when they are created, so the diagrams of machines go first.
    private final Map<String, IStateMachineDiagram> machineDiagrams = new HashMap<>();
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
            if ("association".equals(type(element)) && element.get("class") instanceof String c) {
                associationClasses.put(c, id);
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
            // Ending the transaction, Astah moves the lines between regions next to what the
            // regions hold and shifts the regions behind them; a second one puts things back.
            // A failure here costs only the positions; the project of the first is saved.
            if (!replaced.isEmpty()) {
                TransactionManager.beginTransaction();
                try {
                    for (Map.Entry<INodePresentation, Map<String, Object>> e : replaced.entrySet()) {
                        try {
                            e.getKey().setLocation(new Point2D.Double(num(e.getValue(), "x"), num(e.getValue(), "y")));
                        } catch (Exception x) {
                            drop("presentation");
                        }
                    }
                    TransactionManager.endTransaction();
                } catch (Exception t) {
                    TransactionManager.abortTransaction();
                    drop("presentation");
                    problem("positions of moved shapes", t);
                }
            }
            accessor.save();
        } finally {
            accessor.close();
        }
    }

    private void build() throws Exception {
        basic = accessor.getModelEditorFactory().getBasicModelEditor();
        useCases = accessor.getModelEditorFactory().getUseCaseModelEditor();
        structures = accessor.getModelEditorFactory().getCompositeStructureModelEditor();
        IModel project = accessor.getProject();
        Map<String, Object> model = elements.get(root);
        if (!name(model).isEmpty()) {
            project.setName(name(model));
        }
        common(root, project, Set.of());
        created.put(root, project);

        structure(root);
        for (String id : associationClasses.keySet()) {
            attempt(id, () -> associationClass(id));
        }
        // Before the features, whose types may name them.
        for (String id : templateParameters) {
            attempt(id, () -> templateParameter(id));
        }
        for (Map.Entry<String, Map<String, Object>> e : elements.entrySet()) {
            if ("class".equals(type(e.getValue())) && e.getValue().get("binding") != null) {
                attempt(e.getKey(), () -> bind(e.getKey()));
            }
        }
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
        for (String id : submachinesFirst(diagrams)) {
            attempt(id, () -> diagram(id));
        }
        // Call behavior actions call activities, which exist once their diagrams do.
        for (Map.Entry<IAction, String> c : calls.entrySet()) {
            IActivity target = activities.get(c.getValue());
            if (target == null) {
                drop("calledActivity");
                continue;
            }
            try {
                c.getKey().setCallingActivity(target);
            } catch (Exception x) {
                drop("calledActivity");
                problem("call behavior action \"" + c.getKey().getName() + "\"", x);
            }
        }
        // Astah's interaction uses refer to sequence diagrams.
        for (Map.Entry<IInteractionUse, String> r : references.entrySet()) {
            ISequenceDiagram target = sequenceDiagrams.get(r.getValue());
            if (target == null) {
                drop("interactionUseReference");
                continue;
            }
            try {
                r.getKey().setSequenceDiagram(target);
            } catch (Exception x) {
                drop("interactionUseReference");
                problem("interaction use \"" + r.getKey().getName() + "\"", x);
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
            if ("link".equals(type(e)) && !created.containsKey(e.get("id"))) {
                // Links, too, are made with their presentation.
                drop("link");
            }
            if ("systemBoundary".equals(type(e)) && !shownBoundaries.contains(e.get("id"))) {
                // Astah draws system boundaries as rectangles; they exist only on diagrams.
                drop("systemBoundary");
            }
            if ("connector".equals(type(e)) && !created.containsKey(e.get("id"))) {
                // The API creates connectors with their line on a composite structure diagram.
                drop("connector");
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
            // An association class comes with its association (see `associationClass`).
            if ("class".equals(type) && associationClasses.containsKey(child)) {
                continue;
            }
            switch (type) {
                case "package", "class", "interface", "enumeration", "dataType", "actor", "useCase",
                        "extensionPoint", "enumerationLiteral", "component", "artifact", "node" -> {
                    Object made = attempt(child, () -> element(child, type, owner));
                    if (made != null) {
                        created.put(child, made);
                        structure(child);
                    }
                }
                case "templateParameter" -> templateParameters.add(child);
                case "attribute", "operation", "port" -> features.add(child);
                case "association", "generalization", "realization", "dependency", "include", "extend" ->
                    relationships.add(child);
                // Astah's ER diagrams are of another notation; Chen's has no place there.
                case "diagram" -> {
                    if ("entityRelationship".equals(text(e, "diagram"))) {
                        drop("erDiagram");
                    } else {
                        diagrams.add(child);
                    }
                }
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
                // Comments, instances, links, system boundaries and connectors are created with
                // their presentations on diagrams.
                case "comment", "instance", "link", "systemBoundary", "connector" -> {
                }
                default -> drop(type);
            }
        }
    }

    private Object element(String id, String type, Object owner) throws Exception {
        Map<String, Object> e = elements.get(id);
        Object made = switch (type) {
            // Astah keeps subsystems as a class of their own, not as a stereotype.
            case "package" -> list(e.get("stereotypes")).contains("subsystem")
                ? named(id, n -> basic.createSubsystem((IPackage) owner, n))
                : named(id, n -> basic.createPackage((IPackage) owner, n));
            // A bound class without a name gets one until it is bound (see `bind`); Astah
            // accepts the empty name only then.
            case "class", "dataType" -> e.get("binding") != null && name(e).isEmpty()
                ? (owner instanceof IPackage p ? basic.createClass(p, "bound " + id)
                    : basic.createClass((IClass) owner, "bound " + id))
                : named(id, n -> owner instanceof IPackage p
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
        common(id, (IElement) made, "package".equals(type) ? Set.of("subsystem") : Set.of());
        return made;
    }

    /// An association class: Astah makes the class and its association in one, between the
    /// classes at the association's ends. Its features and nested elements follow as usual; the
    /// ends get their details with the association's other relationships.
    private Object associationClass(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        Map<String, Object> association = elements.get(associationClasses.get(id));
        List<Object> ends = list(association.get("ends"));
        Map<String, Object> end0 = (Map<String, Object>) ends.get(0);
        Map<String, Object> end1 = (Map<String, Object>) ends.get(1);
        IClass a = classifier(end0.get("participant"));
        IClass b = classifier(end1.get("participant"));
        IAssociationClass made = (IAssociationClass) named(id,
            n -> basic.createAssociationClass(a, b, n, text(end0, "role"), text(end1, "role")));
        // Astah puts it where it likes (next to its first end); UmlCraft's owner is lost.
        Object owner = created.get(owners.get(id));
        if (owner != null && made.getOwner() != owner) {
            drop("associationClassOwner");
        }
        made.setAbstract(flag(e, "isAbstract"));
        made.setLeaf(flag(e, "isLeaf"));
        made.setActive(flag(e, "isActive"));
        common(id, made, Set.of());
        created.put(id, made);
        structure(id);
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
            if (owner instanceof IComponent && !(t instanceof IClass)) {
                drop("componentAttribute");
                return;
            }
            IAttribute a = (IAttribute) named(id, n -> owner instanceof IComponent
                ? componentEnd(owner, n, (IClass) t)
                : t instanceof IClass c ? basic.createAttribute(owner, n, c)
                : basic.createAttribute(owner, n, (String) t));
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
        if ("port".equals(type(e))) {
            IPort port = (IPort) named(id, n -> basic.createPort(owner, n));
            if (e.get("typeRef") != null) {
                if (type(e.get("typeRef")) instanceof IClass c) {
                    port.setType(c);
                } else {
                    port.setTypeExpression((String) type(e.get("typeRef")));
                }
            }
            multiplicity(port, (String) e.get("multiplicity"));
            port.setIsBehavior(flag(e, "isBehavior"));
            common(id, port, Set.of());
            created.put(id, port);
            return;
        }
        // An empty type expression makes an operation without a return type ("void" is shown).
        Object r = e.get("returnType") == null ? "" : type(e.get("returnType"));
        IOperation op = (IOperation) named(id, n -> r instanceof IClass c
            ? basic.createOperation(owner, n, c) : basic.createOperation(owner, n, (String) r));
        op.setStatic(flag(e, "isStatic"));
        op.setAbstract(flag(e, "isAbstract"));
        op.setLeaf(flag(e, "isLeaf"));
        if (flag(e, "isQuery")) {
            drop("query");
        }
        common(id, op, Set.of());
        created.put(id, op);
        for (String child : children(id)) {
            attempt(child, () -> parameter(op, child));
        }
    }

    /// A template parameter of a classifier. Astah keeps one type per parameter: the bound of a
    /// type parameter or the type of a value parameter. Operations have no template parameters
    /// in the API; theirs are dropped with the operation's other non-parameter children.
    private Object templateParameter(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        if (!(created.get(owners.get(id)) instanceof IClass owner)) {
            drop("templateParameter");
            return null;
        }
        List<Object> bounds = list(e.get("bounds"));
        Object t = "value".equals(text(e, "parameter"))
            ? type(e.get("typeRef"))
            : bounds.isEmpty() ? null : type(bounds.get(0));
        if (bounds.size() > 1) {
            drop("templateParameterBounds");
        }
        if (flag(e, "isVariadic")) {
            drop("variadicTemplateParameter");
        }
        Object d = e.get("default") == null ? null : type(e.get("default"));
        IClassifierTemplateParameter p = (IClassifierTemplateParameter) named(id, n -> t instanceof IClass c
            ? basic.createTemplateParameter(owner, n, c, d)
            : basic.createTemplateParameter(owner, n, (String) t, d));
        common(id, p, Set.of());
        created.put(id, p);
        return p;
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
            if (m.get("expr") instanceof Map<?, ?>) {
                return typeText(ref);
            }
        }
        return "void";
    }

    /// Binds a bound class to its template, with the arguments in the order of the template's
    /// parameters. Astah takes classifiers and primitive types as arguments, nothing else.
    private Object bind(String id) throws Exception {
        Map<String, Object> e = elements.get(id);
        Map<String, Object> binding = (Map<String, Object>) e.get("binding");
        Map<String, Object> expr = binding.get("expr") instanceof Map<?, ?> x ? (Map<String, Object>) x : null;
        Object template = expr != null && expr.get("base") instanceof Map<?, ?> base
            && base.get("element") instanceof String templateId ? created.get(templateId) : null;
        if (!(template instanceof IClass t) || !(created.get(id) instanceof IClass bound)) {
            drop("binding");
            return null;
        }
        ITemplateBinding made = basic.createTemplateBinding(bound, t);
        IClassifierTemplateParameter[] parameters = t.getTemplateParameters();
        List<Object> arguments = list(expr.get("arguments"));
        for (int i = 0; i < arguments.size(); i++) {
            if (i >= parameters.length) {
                drop("bindingArgument");
                continue;
            }
            try {
                made.addActualParameter(parameters[i], type(arguments.get(i)));
            } catch (InvalidEditingException x) {
                drop("bindingArgument");
            }
        }
        if (name(e).isEmpty()) {
            bound.setName("");
        }
        bindings.put(id, made);
        return made;
    }

    /// A type as written: `List<Foo>`, `Foo[]`, `? extends Number`. Given such a text, the API
    /// binds a template classifier of that name to the arguments itself, through an anonymous
    /// bound classifier, which is how Astah stores bound templates.
    private String typeText(Object ref) {
        if (!(ref instanceof Map<?, ?> m)) {
            return "";
        }
        if (m.get("element") instanceof String target) {
            return name(elements.get(target));
        }
        if (m.get("text") instanceof String text) {
            return text;
        }
        if (!(m.get("expr") instanceof Map<?, ?> e)) {
            return "";
        }
        StringBuilder s = new StringBuilder();
        if (e.get("prefix") instanceof String prefix) {
            s.append(prefix);
        }
        s.append(typeText(e.get("base")));
        if (e.get("arguments") instanceof List<?> arguments && !arguments.isEmpty()) {
            s.append('<');
            for (int i = 0; i < arguments.size(); i++) {
                s.append(i == 0 ? "" : ", ").append(typeText(arguments.get(i)));
            }
            s.append('>');
        }
        if (e.get("modifier") instanceof String modifier) {
            s.append(modifier);
        }
        return s.toString();
    }

    private static void multiplicity(IAttribute a, String text) throws InvalidEditingException {
        if (text == null) {
            return;
        }
        // As text, Astah keeps a lone "*" apart from "0..*".
        a.setMultiplicityString(text.trim());
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
                // An association class exists already, with its class; its name, stereotypes
                // and the rest are the class's, the association adds only its constraints.
                IAssociationClass associationClass = null;
                if (e.get("class") instanceof String c) {
                    if (!(created.get(c) instanceof IAssociationClass ac)) {
                        throw new IllegalStateException("association class " + c + " was not exported");
                    }
                    associationClass = ac;
                    if (!name.isEmpty() || !list(e.get("stereotypes")).isEmpty()
                            || !list(e.get("taggedValues")).isEmpty() || !text(e, "documentation").isEmpty()) {
                        drop("associationClassDetails");
                    }
                }
                IAssociation association = associationClass != null ? associationClass
                    : basic.createAssociation(a, b, name, text(end0, "role"), text(end1, "role"));
                IAttribute[] members = association.getMemberEnds();
                boolean swapped = a != b && members[0].getType() != a;
                IAttribute first = swapped ? members[1] : members[0];
                IAttribute second = swapped ? members[0] : members[1];
                end(first, second, end0);
                end(second, first, end1);
                if (associationClass != null) {
                    constraints(id, associationClass);
                    created.put(id, associationClass);
                    return;
                }
                made = association;
            }
            case "generalization" -> made =
                basic.createGeneralization(classifier(e.get("specific")), classifier(e.get("general")), name);
            // A port provides an interface by realizing it and requires one by using it.
            case "realization" -> made = created.get(e.get("client")) instanceof IPort port
                ? structures.createRealization(port, classifier(e.get("supplier")), name)
                : basic.createRealization(classifier(e.get("client")), classifier(e.get("supplier")), name);
            case "dependency" -> {
                INamedElement client = named(e.get("client"));
                INamedElement supplier = named(e.get("supplier"));
                INamedElement madeClient;
                ITemplateBinding binding = bindings.get(e.get("client"));
                if (client instanceof IPort port && supplier instanceof IClass s) {
                    IUsage usage = structures.createUsage(port, s, name);
                    madeClient = usage.getClient();
                    made = usage;
                    implied = Set.of("use");
                } else if (list(e.get("stereotypes")).contains("bind") && binding != null
                        && binding.getTemplate() == supplier) {
                    // The «bind» line of a bound class is Astah's binding itself.
                    made = binding;
                    implied = Set.of("bind");
                    madeClient = client;
                } else if (list(e.get("stereotypes")).contains("use")
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

    private void end(IAttribute end, IAttribute opposite, Map<String, Object> e) throws InvalidEditingException {
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
        // Astah makes ends private unless told otherwise; UmlCraft leaves out public.
        end.setVisibility(e.get("visibility") instanceof String v ? v : "public");
        end.setChangeable(!flag(e, "isReadOnly"));
        end.setStatic(flag(e, "isStatic"));
        if (!text(e, "defaultValue").isEmpty()) {
            end.setInitialValue(text(e, "defaultValue"));
        }
        for (Object q : list(e.get("qualifiers"))) {
            Map<String, Object> qualifier = (Map<String, Object>) q;
            // The API takes the opposite end and puts the qualifier on this one. It types a new
            // qualifier with a classifier; a type it does not have is set as an expression
            // afterwards.
            Object t = qualifier.get("typeRef") == null ? null : type(qualifier.get("typeRef"));
            IAttribute made = basic.createQualifier(opposite, text(qualifier, "name"),
                t instanceof IClass c ? c : end.getType());
            if (t instanceof String s) {
                made.setTypeExpression(s);
            }
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
        DiagramEditor current;
        // Sequence diagrams are moved down to Astah's frame.
        double dy = 0;
        switch (kind) {
            case "compositeStructure" -> {
                CompositeStructureDiagramEditor cs =
                    accessor.getDiagramEditorFactory().getCompositeStructureDiagramEditor();
                diagram = (IDiagram) named(id, n -> cs.createCompositeStructureDiagram((INamedElement) owner, n));
                frame(id, diagram);
                compositeDiagram(id, cs);
                current = cs;
            }
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
                frame(id, diagram);
                structureDiagram(id, editor);
                current = editor;
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
                d.setProperty("flat_message_index",
                    Boolean.toString(Boolean.TRUE.equals(props.get("flatMessageNumbers"))));
                Sequence sequence = new Sequence(id, sd);
                sequence.build();
                dy = sequence.dy;
                if (Boolean.TRUE.equals(props.get("hideFrame"))) {
                    for (IPresentation p : d.getPresentations()) {
                        if ("Frame".equals(p.getType())) {
                            p.setProperty("visibility", "false");
                        }
                    }
                }
                diagram = d;
                current = sd;
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
                activities.put(activity, d.getActivity());
                frame(id, d);
                activityDiagram(id, activity, ad);
                diagram = d;
                current = ad;
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
                machineDiagrams.put(machine, d);
                frame(id, d);
                stateDiagram(id, machine, sm);
                diagram = d;
                current = sm;
            }
            default -> throw new IllegalStateException(kind);
        }
        freeShapes(id, current, dy);
        common(id, diagram, Set.of());
        return diagram;
    }

    /// A free line from `from` to `to`. Astah adds the line but then reports an exception from
    /// the command that adds it; the new line is then the presentation the diagram did not have.
    private static IPresentation createLine(DiagramEditor editor, Point2D from, Point2D to) throws Exception {
        Set<IPresentation> before = new HashSet<>(List.of(editor.getDiagram().getPresentations()));
        try {
            return editor.createLine(from, to);
        } catch (InvalidEditingException e) {
            for (IPresentation p : editor.getDiagram().getPresentations()) {
                if (!before.contains(p)) {
                    return p;
                }
            }
            throw e;
        }
    }

    /// Free texts and lines, which show no model element, moved down by `dy`.
    private void freeShapes(String diagram, DiagramEditor editor, double dy) {
        for (Map<String, Object> p : view(diagram)) {
            String shape = String.valueOf(p.get("shape"));
            if ("text".equals(shape)) {
                Map<String, Object> r = (Map<String, Object>) p.get("bounds");
                attempt(null, () -> {
                    return editor.createText(text(p, "text"), new Point2D.Double(num(r, "x"), num(r, "y") + dy));
                });
            } else if ("line".equals(shape)) {
                Map<String, Object> from = (Map<String, Object>) p.get("from");
                Map<String, Object> to = (Map<String, Object>) p.get("to");
                attempt(null, () -> {
                    IPresentation line = createLine(editor,
                        new Point2D.Double(num(from, "x"), num(from, "y") + dy),
                        new Point2D.Double(num(to, "x"), num(to, "y") + dy));
                    if (Boolean.TRUE.equals(p.get("dashed"))) {
                        line.setProperty("line.type", "dash1");
                    }
                    boolean start = Boolean.TRUE.equals(p.get("startArrow"));
                    boolean end = Boolean.TRUE.equals(p.get("endArrow"));
                    if (start || end) {
                        line.setProperty("line.arrow.type",
                            start && end ? "botharrow" : start ? "startarrow" : "endarrow");
                    }
                    return line;
                });
            }
        }
    }

    /// Astah frames new diagrams; UmlCraft only those that show themselves as a node.
    private void frame(String id, IDiagram diagram) throws Exception {
        boolean framed = view(id).stream()
            .anyMatch(p -> "node".equals(p.get("shape")) && id.equals(p.get("element")));
        boolean frame = false;
        for (IPresentation p : diagram.getPresentations()) {
            if ("Frame".equals(p.getType())) {
                p.setProperty("visibility", Boolean.toString(framed));
                frame = true;
            }
        }
        // Composite structure diagrams come without a frame, and the API makes none.
        if (framed && !frame) {
            drop("diagramFrame");
        }
    }

    /// Nodes in z-order, then edges and note anchors of a class or use case diagram.
    private void structureDiagram(String diagram, StructureDiagramEditor editor) throws Exception {
        structureDiagram(diagram, editor, new HashMap<>());
    }

    /// The same, leaving out the presentations in `shown`, which are drawn already.
    private void structureDiagram(String diagram, StructureDiagramEditor editor, Map<String, IPresentation> shown)
            throws Exception {
        List<Map<String, Object>> presentations = view(diagram);
        for (Map<String, Object> p : presentations) {
            if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)
                    || element.equals(diagram) || shown.containsKey(p.get("id"))) {
                continue;
            }
            Map<String, Object> r = rect(diagram, p);
            Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
            String type = type(elements.get(element));
            INodePresentation node;
            if ("comment".equals(type)) {
                node = (INodePresentation) attempt(element, () -> note(editor, element, at));
            } else if ("systemBoundary".equals(type)) {
                attempt(element, () -> systemBoundary(editor, element, r, (Map<String, Object>) p.get("style")));
                continue;
            } else if ("instance".equals(type) && editor instanceof ClassDiagramEditor cd) {
                node = (INodePresentation) attempt(element, () -> instance(cd, element, at));
            } else if (associationClasses.containsKey(element) && editor instanceof ClassDiagramEditor cd
                    && associationLine(diagram, element, shown) != null) {
                // Drawn with its association's line, once both ends are there.
                continue;
            } else if (created.get(element) instanceof IElement model
                    && !(model instanceof IAttribute) && !(model instanceof IOperation)) {
                node = (INodePresentation) attempt(element, () -> editor.createNodePresentation(model, at));
            } else {
                continue;
            }
            if (node == null) {
                continue;
            }
            decorate(diagram, node, p, type);
            shown.put((String) p.get("id"), node);
        }
        if (editor instanceof ClassDiagramEditor cd) {
            associationClassShapes(cd, diagram, presentations, shown);
        }
        edges(editor, diagram, presentations, shown);
    }

    /// The looks, place and size of a node just created. The shape stands; a failure in its
    /// looks costs only them, not the diagram.
    private void decorate(String diagram, INodePresentation shape, Map<String, Object> p, String type) {
        Map<String, Object> r = rect(diagram, p);
        attempt((String) p.get("element"), () -> {
            // Switching the notation resets the size, so the style goes first.
            style(shape, p, type);
            // Instances get their place when they are created and cannot be moved afterwards.
            if (!"instance".equals(type)) {
                shape.setLocation(new Point2D.Double(num(r, "x"), num(r, "y")));
            }
            resize(shape, num(r, "width"), num(r, "height"));
            // Boxes sized by their content stay so in Astah, which sizes them by the same rule.
            if (p.get("style") instanceof Map<?, ?> s && Boolean.TRUE.equals(s.get("autoSize"))
                    && shape.getProperties().containsKey("auto_resize")) {
                shape.setProperty("auto_resize", "true");
            }
            return null;
        });
    }

    /// The line of the association of an association class on the diagram, if both its ends
    /// are drawn.
    private Map<String, Object> associationLine(String diagram, String associationClass,
            Map<String, IPresentation> shown) {
        String association = associationClasses.get(associationClass);
        Set<String> nodes = new HashSet<>();
        for (Map<String, Object> p : view(diagram)) {
            if ("node".equals(p.get("shape"))) {
                nodes.add((String) p.get("id"));
            }
        }
        for (Map<String, Object> p : view(diagram)) {
            if ("edge".equals(p.get("shape")) && association.equals(p.get("element"))
                    && nodes.contains(p.get("source")) && nodes.contains(p.get("target"))) {
                return p;
            }
        }
        return null;
    }

    /// Association classes and their lines: Astah draws the box, the line and the dashed line
    /// between them in one.
    private void associationClassShapes(ClassDiagramEditor editor, String diagram,
            List<Map<String, Object>> presentations, Map<String, IPresentation> shown) {
        for (Map<String, Object> p : presentations) {
            if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)
                    || !associationClasses.containsKey(element) || shown.containsKey(p.get("id"))) {
                continue;
            }
            Map<String, Object> line = associationLine(diagram, element, shown);
            if (line == null || shown.containsKey(line.get("id"))) {
                continue;
            }
            IPresentation source = shown.get(line.get("source"));
            IPresentation target = shown.get(line.get("target"));
            if (!(created.get(element) instanceof IAssociationClass model)
                    || !(source instanceof INodePresentation s) || !(target instanceof INodePresentation t)) {
                drop("presentation");
                continue;
            }
            IPresentation[] made = (IPresentation[]) attempt(element,
                () -> editor.createAssociationClassPresentation(model, s, t));
            if (made == null) {
                continue;
            }
            // The API hands back the box and the association's line.
            for (IPresentation part : made) {
                if (part instanceof INodePresentation node && !shown.containsKey(p.get("id"))) {
                    decorate(diagram, node, p, "class");
                    shown.put((String) p.get("id"), node);
                } else if (part instanceof ILinkPresentation link && !shown.containsKey(line.get("id"))) {
                    attempt(element, () -> {
                        lineDetails(link, s, t, diagram, line);
                        return null;
                    });
                    shown.put((String) line.get("id"), link);
                }
            }
        }
    }

    // ----- Composite structure diagrams -------------------------------------------------------

    /// Structured classes first, then the parts drawn in them and the ports on their borders;
    /// the interfaces ports provide or require as circles and sockets, which Astah draws with
    /// their lines itself; then the other nodes and the lines.
    private void compositeDiagram(String diagram, CompositeStructureDiagramEditor editor) throws Exception {
        Map<String, IPresentation> shown = new HashMap<>();
        // Where each drawn node lies in UmlCraft, to find what a part or port sits in or on.
        Map<String, Map<String, Object>> rects = new HashMap<>();
        List<Map<String, Object>> nodes = new ArrayList<>();
        for (Map<String, Object> p : view(diagram)) {
            if ("node".equals(p.get("shape")) && p.get("element") instanceof String element
                    && !element.equals(diagram)) {
                nodes.add(p);
            }
        }
        for (String pass : List.of("structured", "part", "port")) {
            for (Map<String, Object> p : nodes) {
                String element = (String) p.get("element");
                String type = type(elements.get(element));
                boolean structured = Set.of("class", "component").contains(type);
                if (!(structured && "structured".equals(pass) || "attribute".equals(type) && "part".equals(pass)
                        || "port".equals(type) && "port".equals(pass))) {
                    continue;
                }
                Map<String, Object> r = rect(diagram, p);
                Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
                Object made = attempt(element, () -> {
                    IElement model = (IElement) created.get(element);
                    if (model == null) {
                        return null;
                    }
                    if (structured) {
                        return editor.createStructuredClassPresentation(model, at);
                    }
                    // A part lies in a class drawn for its owner, a port on the border nearest to it.
                    INodePresentation host = "port".equals(type)
                        ? nearestBorder(shown, rects, r)
                        : around(shown, rects, r, created.get(owners.get(element)));
                    if (host == null) {
                        drop("port".equals(type) ? "portOutsideHost" : "partOutsideClass");
                        return null;
                    }
                    return "port".equals(type)
                        ? editor.createPortPresentation(host, (IPort) model, at)
                        : editor.createStructuredClassPresentation(model, host, at);
                });
                if (!(made instanceof INodePresentation node)) {
                    continue;
                }
                if ("attribute".equals(type) && name(elements.get(element)).isEmpty()) {
                    unnamePart((IAttribute) node.getModel());
                }
                attempt(element, () -> {
                    node.setLocation(at);
                    if (!"port".equals(type)) {
                        resize(node, num(r, "width"), num(r, "height"));
                    }
                    String fill = ownFill((Map<String, Object>) p.get("style"));
                    node.setProperty("fill.color", fill != null ? fill : DEFAULT_FILL);
                    return null;
                });
                shown.put((String) p.get("id"), node);
                rects.put((String) p.get("id"), r);
            }
        }
        portInterfaces(diagram, editor, shown);
        structureDiagram(diagram, editor, shown);
    }

    /// A part may have no name (`: Engine`). Astah creates its attribute with a numbered name;
    /// shown as a part, it may give the name up again.
    private void unnamePart(IAttribute part) {
        for (String name : new String[] {"", " "}) {
            try {
                part.setName(name);
                dropped.computeIfPresent("renamed", (k, n) -> n > 1 ? n - 1 : null);
                return;
            } catch (InvalidEditingException x) {
                // It keeps the numbered name.
            }
        }
    }

    /// Astah gives components no attributes, only association ends. A part of a component becomes
    /// the far end of an association to its type, the way Astah keeps parts it draws itself.
    private IAttribute componentEnd(IClass owner, String name, IClass type) throws InvalidEditingException {
        IAssociation association = basic.createAssociation(owner, type, "", "", name);
        // Without the diamond at the whole, Astah draws the part dashed, as a reference.
        association.getMemberEnds()[0].setComposite();
        return association.getMemberEnds()[1];
    }

    /// The drawn class of `owner` around the box `r`.
    private static INodePresentation around(Map<String, IPresentation> shown, Map<String, Map<String, Object>> rects,
            Map<String, Object> r, Object owner) {
        double cx = num(r, "x") + num(r, "width") / 2;
        double cy = num(r, "y") + num(r, "height") / 2;
        for (Map.Entry<String, IPresentation> e : shown.entrySet()) {
            Map<String, Object> h = rects.get(e.getKey());
            if (e.getValue().getModel() == owner && h != null && num(h, "x") <= cx && cx <= num(h, "x") + num(h, "width")
                    && num(h, "y") <= cy && cy <= num(h, "y") + num(h, "height")) {
                return (INodePresentation) e.getValue();
            }
        }
        return null;
    }

    /// The drawn class or part whose border is nearest to the center of the box `r`.
    private static INodePresentation nearestBorder(Map<String, IPresentation> shown,
            Map<String, Map<String, Object>> rects, Map<String, Object> r) {
        double cx = num(r, "x") + num(r, "width") / 2;
        double cy = num(r, "y") + num(r, "height") / 2;
        INodePresentation best = null;
        double bestDistance = Double.MAX_VALUE;
        for (Map.Entry<String, IPresentation> e : shown.entrySet()) {
            Map<String, Object> h = rects.get(e.getKey());
            if (h == null) {
                continue;
            }
            double left = num(h, "x"), top = num(h, "y");
            double right = left + num(h, "width"), bottom = top + num(h, "height");
            double dx = Math.max(Math.max(left - cx, cx - right), 0);
            double dy = Math.max(Math.max(top - cy, cy - bottom), 0);
            double distance = dx > 0 || dy > 0 ? Math.hypot(dx, dy)
                : Math.min(Math.min(cx - left, right - cx), Math.min(cy - top, bottom - cy));
            if (distance < bestDistance) {
                bestDistance = distance;
                best = (INodePresentation) e.getValue();
            }
        }
        return best;
    }

    /// Interfaces drawn as circles or sockets at a port. Astah draws them only together with
    /// their lines, for all realizations and usages of the port at once; each goes where
    /// UmlCraft has it, and those UmlCraft does not draw are removed again.
    private void portInterfaces(String diagram, CompositeStructureDiagramEditor editor,
            Map<String, IPresentation> shown) {
        List<Map<String, Object>> presentations = view(diagram);
        Map<String, Map<String, Object>> byId = new HashMap<>();
        for (Map<String, Object> p : presentations) {
            byId.put((String) p.get("id"), p);
        }
        // The lines from each drawn port to an interface drawn as an icon.
        Map<String, List<Map<String, Object>>> lines = new LinkedHashMap<>();
        for (Map<String, Object> p : presentations) {
            if (!"edge".equals(p.get("shape")) || !(p.get("element") instanceof String element)
                    || !Set.of("realization", "dependency").contains(type(elements.get(element)))) {
                continue;
            }
            Map<String, Object> icon = byId.get(p.get("target"));
            boolean iconic = icon != null && icon.get("style") instanceof Map<?, ?> s
                && Boolean.TRUE.equals(s.get("iconic"));
            if (iconic && shown.get(p.get("source")) instanceof INodePresentation port
                    && port.getModel() instanceof IPort) {
                lines.computeIfAbsent((String) p.get("source"), k -> new ArrayList<>()).add(p);
            }
        }
        for (Map.Entry<String, List<Map<String, Object>>> e : lines.entrySet()) {
            INodePresentation port = (INodePresentation) shown.get(e.getKey());
            attempt(e.getKey(), () -> {
                Set<IPresentation> before = new HashSet<>(List.of(editor.getDiagram().getPresentations()));
                editor.showInterfacePresentations(port, port.getLocation());
                List<IPresentation> added = new ArrayList<>();
                for (IPresentation p : editor.getDiagram().getPresentations()) {
                    if (!before.contains(p)) {
                        added.add(p);
                    }
                }
                Set<IPresentation> kept = new HashSet<>();
                for (Map<String, Object> line : e.getValue()) {
                    Object model = created.get(line.get("element"));
                    for (IPresentation p : added) {
                        if (p instanceof ILinkPresentation link && link.getModel() == model) {
                            INodePresentation icon = link.getSource() == port
                                ? (INodePresentation) link.getTarget() : (INodePresentation) link.getSource();
                            Map<String, Object> target = byId.get(line.get("target"));
                            Map<String, Object> r = rect(diagram, target);
                            icon.setLocation(new Point2D.Double(num(r, "x"), num(r, "y")));
                            shown.put((String) line.get("target"), icon);
                            shown.put((String) line.get("id"), link);
                            kept.add(link);
                            kept.add(icon);
                        }
                    }
                }
                for (IPresentation p : added) {
                    if (!kept.contains(p) && p instanceof INodePresentation) {
                        editor.deletePresentation(p);
                    }
                }
                return null;
            });
        }
    }

    /// A connector between parts and ports, with the name and without the arrow that shows
    /// which way to read it.
    private ILinkPresentation connector(CompositeStructureDiagramEditor editor, String id,
            INodePresentation source, INodePresentation target) throws Exception {
        Map<String, Object> e = elements.get(id);
        ILinkPresentation link = editor.createConnectorPresentation(source, target, name(e));
        link.setProperty("name_direction_visibility", "false");
        for (Object end : list(e.get("ends"))) {
            if (end instanceof Map<?, ?> m && m.get("multiplicity") != null) {
                drop("connectorEndMultiplicity");
            }
        }
        IConnector connector = (IConnector) link.getModel();
        common(id, connector, Set.of());
        created.put(id, connector);
        return link;
    }

    // ----- Activity diagrams ------------------------------------------------------------------

    /// Partitions first, since Astah lines up the lanes itself; then the other nodes, the flows
    /// and the note anchors. Nodes, flows and partitions of the activity that the diagram does
    /// not show are lost: Astah creates them only on a diagram.
    private void activityDiagram(String diagram, String activity, ActivityDiagramEditor editor) throws Exception {
        Map<String, IPresentation> shown = new HashMap<>();
        List<Map<String, Object>> presentations = view(diagram);
        partitions(diagram, activity, null, editor, presentations, shown);
        // Pins after the actions they sit on.
        for (boolean pins : new boolean[] {false, true}) {
            for (Map<String, Object> p : presentations) {
                if (!"node".equals(p.get("shape")) || !(p.get("element") instanceof String element)
                        || created.containsKey(element) && !"comment".equals(type(elements.get(element)))
                        || isPin(elements.get(element)) != pins) {
                    continue;
                }
                Map<String, Object> r = rect(diagram, p);
                Point2D at = new Point2D.Double(num(r, "x"), num(r, "y"));
                Map<String, Object> e = elements.get(element);
                INodePresentation node;
                if ("comment".equals(type(e))) {
                    node = note(editor, element, at);
                } else if (pins) {
                    INodePresentation action = actionOf(element, presentations, shown);
                    if (action == null) {
                        drop("pin");
                        continue;
                    }
                    node = (INodePresentation) attempt(element, () -> pin(editor, element, action, at));
                } else if ("activityNode".equals(type(e))) {
                    node = (INodePresentation) attempt(element, () -> activityNode(editor, element, at));
                } else {
                    continue;
                }
                if (node == null) {
                    continue;
                }
                try {
                    node.setLocation(at);
                } catch (InvalidEditingException x) {
                    drop("position");
                }
                // Astah sizes pins and connectors itself.
                if (!pins && !"connector".equals(text(e, "node"))) {
                    resize(node, num(r, "width"), num(r, "height"));
                }
                fill(node, p);
                shown.put((String) p.get("id"), node);
            }
        }
        edges(editor, diagram, presentations, shown);
        for (String child : children(activity)) {
            String type = type(elements.get(child));
            if (!created.containsKey(child) && !"diagram".equals(type)) {
                drop(type);
            }
            // Pins not shown with their action.
            for (String pin : children(child)) {
                if (!created.containsKey(pin) && isPin(elements.get(pin))) {
                    drop("pin");
                }
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
            try {
                placeLanes(diagram, lanes, shown);
            } catch (Exception x) {
                // The partitions stand, only not where they were.
                drop("partition");
                problem("partitions of \"" + name(elements.get(owner)) + "\"", x);
            }
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

    private static boolean isPin(Map<String, Object> e) {
        return e != null && "activityNode".equals(type(e))
            && ("inputPin".equals(text(e, "node")) || "outputPin".equals(text(e, "node")));
    }

    /// The node of the action the pin `id` belongs to on this diagram, the one nearest the pin.
    private INodePresentation actionOf(String id, List<Map<String, Object>> presentations,
            Map<String, IPresentation> shown) {
        String action = owners.get(id);
        for (Map<String, Object> p : presentations) {
            if (action != null && action.equals(p.get("element"))
                    && shown.get(p.get("id")) instanceof INodePresentation node) {
                return node;
            }
        }
        return null;
    }

    /// A pin on `action`, with the object node's type and properties.
    private INodePresentation pin(ActivityDiagramEditor editor, String id, INodePresentation action, Point2D at)
            throws Exception {
        Map<String, Object> e = elements.get(id);
        IClass base = objectType(e);
        boolean input = "inputPin".equals(text(e, "node"));
        INodePresentation pin =
            (INodePresentation) named(id, n -> editor.createPin(n, base, input, action, at));
        objectProperties(e, (IObjectNode) pin.getModel());
        created.put(id, pin.getModel());
        common(id, (IElement) pin.getModel(), Set.of());
        return pin;
    }

    /// The class an object node, pin or activity parameter is typed by; another type is
    /// reported, as Astah types them by classes only.
    private IClass objectType(Map<String, Object> e) {
        Object t = e.get("typeRef") == null ? null : type(e.get("typeRef"));
        IClass base = t instanceof IClass c ? c : null;
        if (t != null && base == null) {
            drop("objectNodeType");
        }
        return base;
    }

    /// State, ordering, upper bound, control, effect, stream and exception of an object node,
    /// pin or activity parameter.
    private void objectProperties(Map<String, Object> e, IObjectNode node) throws InvalidEditingException {
        if (!text(e, "inState").isEmpty()) {
            node.setInState(text(e, "inState"));
        }
        // UmlCraft's names of the orderings and Astah's.
        String ordering = switch (text(e, "ordering")) {
            case "unordered" -> "unordered";
            case "ordered" -> "ordered";
            case "lifo" -> "LIFO";
            case "fifo" -> "FIFO";
            default -> null;
        };
        if (ordering != null) {
            node.setOrdering(ordering);
        }
        if (!text(e, "upperBound").isEmpty()) {
            node.setUpperBound(text(e, "upperBound"));
        }
        if (flag(e, "isControl")) {
            node.setControl(true);
        }
        // UmlCraft's effects are Astah's, which takes them in lower case only.
        if (!text(e, "effect").isEmpty()) {
            node.setEffect(text(e, "effect"));
        }
        if (flag(e, "isStream")) {
            node.setStream("stream");
        }
        if (flag(e, "isException")) {
            node.setException(true);
        }
    }

    private INodePresentation activityNode(ActivityDiagramEditor editor, String id, Point2D at) throws Exception {
        Map<String, Object> e = elements.get(id);
        INodePresentation node = switch (text(e, "node")) {
            case "", "action" -> (INodePresentation) named(id, n -> editor.createAction(n, at));
            case "sendSignal" -> (INodePresentation) named(id, n -> editor.createSendSignalAction(n, at));
            case "acceptEvent" -> (INodePresentation) named(id, n -> editor.createAcceptEventAction(n, at));
            case "acceptTimeEvent" ->
                (INodePresentation) named(id, n -> editor.createAcceptTimeEventAction(n, at));
            case "callBehavior" -> {
                // The activity called is set once every activity exists.
                INodePresentation call =
                    (INodePresentation) named(id, n -> editor.createCallBehaviorAction(n, null, at));
                if (e.get("behavior") instanceof String called) {
                    calls.put((IAction) call.getModel(), called);
                }
                yield call;
            }
            case "connector" -> (INodePresentation) named(id, n -> editor.createConnector(n, at));
            case "activityParameter" -> {
                IClass base = objectType(e);
                INodePresentation parameter =
                    (INodePresentation) named(id, n -> editor.createActivityParameterNode(n, base, at));
                objectProperties(e, (IObjectNode) parameter.getModel());
                yield parameter;
            }
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
                IClass base = objectType(e);
                INodePresentation object =
                    (INodePresentation) named(id, n -> editor.createObjectNode(n, base, at));
                objectProperties(e, (IObjectNode) object.getModel());
                yield object;
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
            } else if (unmade(e)) {
                // The API makes no entry and exit points and no stub states; they are counted
                // below, with what the diagram does not show.
                continue;
            } else if (Set.of("state", "finalState", "pseudostate").contains(type(e)) && !created.containsKey(element)) {
                // In a region, the vertex goes into the state that holds the regions, created
                // first inside the right region (Astah picks it by place) and then moved.
                String owner = owners.get(element);
                int region = -1;
                if ("region".equals(type(elements.get(owner)))) {
                    region = regionsOf(owners.get(owner)).indexOf(owner);
                    owner = owners.get(owner);
                }
                INodePresentation parent = nodeOf.get(owner);
                Point2D start = parent != null && region >= 0 ? inRegion(parent, region, at) : at;
                node = (INodePresentation) attempt(element, () -> vertex(editor, element, parent, start, r));
            } else {
                continue;
            }
            if (node == null) {
                continue;
            }
            node.setLocation(at);
            // Astah keeps room for entry, do and exit, which makes a state at least 81 high;
            // a state without them does not need it.
            // Submachine states keep no such room.
            if ("state".equals(type(e)) && text(e, "entry").isEmpty() && text(e, "doActivity").isEmpty()
                    && text(e, "exit").isEmpty() && !(node.getModel() instanceof IState s && s.isSubmachineState())) {
                node.setProperty("action_visibility", "false");
            }
            resize(node, num(r, "width"), num(r, "height"));
            fill(node, p);
            shown.put((String) p.get("id"), node);
            nodeOf.putIfAbsent(element, node);
            if ("state".equals(type(e))) {
                INodePresentation state = node;
                attempt(element, () -> regions(editor, element, state, p));
            }
        }
        // Astah grows orthogonal states and their regions around what goes into them and moves
        // things on the way; once everything is there, it all goes back to its place, from the
        // outside in.
        for (Map<String, Object> p : presentations) {
            if (shown.get(p.get("id")) instanceof INodePresentation node
                    && p.get("element") instanceof String element
                    && Set.of("state", "finalState", "pseudostate").contains(type(elements.get(element)))
                    && inOrthogonalState(element)) {
                Map<String, Object> r = rect(diagram, p);
                node.setLocation(new Point2D.Double(num(r, "x"), num(r, "y")));
                resize(node, num(r, "width"), num(r, "height"));
                replaced.put(node, r);
            }
        }
        edges(editor, diagram, view(diagram), shown);
        for (String child : subtree(machine)) {
            String type = type(elements.get(child));
            if (!created.containsKey(child)
                    && Set.of("state", "finalState", "pseudostate", "transition", "internalTransition").contains(type)) {
                drop(unmade(elements.get(child)) ? text(elements.get(child), "pseudostate") : type);
            }
        }
    }

    /// Whether `e` is a vertex the API cannot make: an entry or exit point, a stub state.
    private static boolean unmade(Map<String, Object> e) {
        return "pseudostate".equals(type(e))
            && Set.of("entryPoint", "exitPoint", "stubState").contains(text(e, "pseudostate"));
    }

    /// `diagrams` with the diagram of every state machine a submachine state stands for before
    /// the diagram of that state, as far as they do not refer to each other in a circle.
    private List<String> submachinesFirst(List<String> diagrams) {
        Map<String, String> diagramOf = new HashMap<>();
        for (String d : diagrams) {
            diagramOf.putIfAbsent(owners.get(d), d);
        }
        List<String> result = new ArrayList<>();
        Set<String> visited = new HashSet<>();
        for (String d : diagrams) {
            submachinesFirst(d, diagramOf, visited, result);
        }
        return result;
    }

    private void submachinesFirst(String diagram, Map<String, String> diagramOf, Set<String> visited,
            List<String> result) {
        if (!visited.add(diagram)) {
            return;
        }
        String machine = owners.get(diagram);
        if ("stateMachine".equals(text(elements.get(diagram), "diagram")) && machine != null) {
            for (String child : subtree(machine)) {
                String target = diagramOf.get(text(elements.get(child), "submachine"));
                if ("state".equals(type(elements.get(child))) && target != null) {
                    submachinesFirst(target, diagramOf, visited, result);
                }
            }
        }
        result.add(diagram);
    }

    private INodePresentation vertex(StateMachineDiagramEditor editor, String id, INodePresentation parent, Point2D at,
            Map<String, Object> r) throws Exception {
        Map<String, Object> e = elements.get(id);
        INodePresentation node = switch (type(e)) {
            case "state" -> {
                // A submachine state needs the diagram of its machine, made before this one; a
                // state whose machine has none (or comes later) loses the reference.
                String submachine = text(e, "submachine");
                IStateMachineDiagram target = machineDiagrams.get(submachine);
                if (!submachine.isEmpty() && target == null) {
                    drop("submachine");
                }
                yield target != null
                    ? editor.createSubmachineState(parent, target, at)
                    : (INodePresentation) named(id, n -> editor.createState(n, parent, at));
            }
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
            for (String child : children(id)) {
                Map<String, Object> t = elements.get(child);
                if ("internalTransition".equals(type(t))) {
                    state.addInternalTransition(text(t, "trigger"), text(t, "guard"), text(t, "effect"));
                    ITransition[] all = state.getInternalTransitions();
                    created.put(child, all[all.length - 1]);
                }
            }
        }
        created.put(id, model);
        common(id, model, Set.of());
        return node;
    }

    /// The regions of an orthogonal state. The API divides a state once, into two regions laid
    /// out one below the other or side by side; further regions are dropped, and what they
    /// hold goes into the second one. Astah places the line between them itself, below or
    /// beside what the first region holds.
    private Object regions(StateMachineDiagramEditor editor, String state, INodePresentation node,
            Map<String, Object> p) throws Exception {
        List<String> regions = regionsOf(state);
        if (regions.isEmpty()) {
            return null;
        }
        boolean vertical = p.get("regions") instanceof Map<?, ?> layout && Boolean.TRUE.equals(layout.get("vertical"));
        if (regions.size() > 1) {
            editor.addRegion(node, !vertical);
        }
        for (int i = 2; i < regions.size(); i++) {
            drop("region");
        }
        List<IState> made = ((IState) node.getModel()).getStateRegions();
        for (int i = 0; i < regions.size() && i < made.size(); i++) {
            created.put(regions.get(i), made.get(i));
            String name = name(elements.get(regions.get(i)));
            if (!name.isEmpty()) {
                made.get(i).setName(name);
            }
        }
        return null;
    }

    /// Whether `vertex` has regions or lies in a region.
    private boolean inOrthogonalState(String vertex) {
        if (!regionsOf(vertex).isEmpty()) {
            return true;
        }
        for (String a = owners.get(vertex); a != null; a = owners.get(a)) {
            if ("region".equals(type(elements.get(a)))) {
                return true;
            }
        }
        return false;
    }

    /// The regions of `state`, in order.
    private List<String> regionsOf(String state) {
        List<String> result = new ArrayList<>();
        for (String child : children(state)) {
            if ("region".equals(type(elements.get(child)))) {
                result.add(child);
            }
        }
        return result;
    }

    /// Where a new vertex meant for `at` lands in region `index` of the state drawn as `parent`
    /// (the last one Astah has, for regions it could not make): `at` if it lies in the region,
    /// else the region's middle. Astah grows a state around a vertex placed near its edge.
    private static Point2D inRegion(INodePresentation parent, int index, Point2D at) throws Exception {
        IState state = (IState) parent.getModel();
        if (state.getRegionSize() == 0) {
            return at;
        }
        java.awt.geom.Rectangle2D r = state.getRegionRectangle(Math.min(index, state.getRegionSize() - 1));
        return r.contains(at) ? at : new Point2D.Double(r.getCenterX(), r.getCenterY());
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
        String own = ownFill((Map<String, Object>) p.get("style"));
        if (own != null) {
            node.setProperty("fill.color", own);
        }
    }

    /// The fill color a style sets, as Astah writes it (`#RRGGBB`, upper case, no alpha), or
    /// null.
    private static String ownFill(Map<String, Object> style) {
        return style != null && style.get("fill") instanceof String f && f.length() >= 7
            ? f.substring(0, 7).toUpperCase() : null;
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
        // Astah gives the instance a slot per attribute of its classifier; set the values.
        for (String child : children(id)) {
            Map<String, Object> s = elements.get(child);
            if ("slot".equals(type(s))) {
                attempt(child, () -> slot(instance, s));
            }
        }
        return node;
    }

    private Object slot(IInstanceSpecification instance, Map<String, Object> s) throws Exception {
        Map<String, Object> attribute =
            s.get("attribute") instanceof String a ? elements.get(a) : null;
        ISlot slot = instance.getSlot(attribute != null ? name(attribute) : name(s));
        if (slot == null) {
            drop("slot");
            return null;
        }
        slot.setValue(text(s, "value"));
        return slot;
    }

    /// A link between two objects. Its ends follow the objects; the API cannot say which
    /// association the link instantiates.
    private ILinkPresentation link(ClassDiagramEditor editor, String id, INodePresentation source,
            INodePresentation target) throws Exception {
        Map<String, Object> e = elements.get(id);
        ILinkPresentation presentation = editor.createInstanceSpecificationLink(source, target);
        ILink link = (ILink) presentation.getModel();
        if (!name(e).isEmpty()) {
            link.setName(name(e));
        }
        for (Object o : list(e.get("ends"))) {
            Map<String, Object> end = (Map<String, Object>) o;
            Object object = created.get(end.get("instance"));
            for (ILinkEnd made : link.getMemberEnds()) {
                if (made.getType() == object) {
                    linkEnd(made, end);
                    break;
                }
            }
        }
        if (e.get("association") != null) {
            drop("linkAssociation");
        }
        created.put(id, link);
        common(id, link, Set.of());
        return presentation;
    }

    private static void linkEnd(ILinkEnd end, Map<String, Object> e) throws InvalidEditingException {
        if (!text(e, "role").isEmpty()) {
            end.setName(text(e, "role"));
        }
        switch (text(e, "navigability")) {
            case "navigable" -> end.setNavigability("Navigable");
            case "nonNavigable" -> end.setNavigability("Non_Navigable");
            default -> {
            }
        }
        switch (text(e, "aggregation")) {
            case "shared" -> end.setAggregation(true);
            case "composite" -> end.setComposite(true);
            default -> {
            }
        }
    }

    private INodePresentation note(BasicDiagramEditor editor, String comment, Point2D at) throws Exception {
        INodePresentation note = editor.createNote(text(elements.get(comment), "body"), at);
        shownComments.add(comment);
        common(comment, note.getModel(), Set.of());
        return note;
    }

    /// Astah has no system boundary; it is drawn as a rectangle with the name as a text,
    /// centered, its top 7 below the rectangle's (the baseline then lies 24 below).
    private INodePresentation systemBoundary(
        BasicDiagramEditor editor, String boundary, Map<String, Object> r, Map<String, Object> style
    ) throws Exception {
        shownBoundaries.add(boundary);
        double x = num(r, "x"), y = num(r, "y"), width = num(r, "width");
        INodePresentation rect = editor.createRect(new Point2D.Double(x, y), width, num(r, "height"));
        String name = name(elements.get(boundary));
        if (!name.isEmpty()) {
            INodePresentation text = editor.createText(name, new Point2D.Double(x, y + BOUNDARY_NAME_TOP));
            // Where the text stood in Astah, or centered.
            if (style != null && style.get("nameOffset") instanceof Map<?, ?> at) {
                text.setLocation(new Point2D.Double(x + num((Map<String, Object>) at, "x"), y + num((Map<String, Object>) at, "y")));
            } else {
                text.setLocation(new Point2D.Double(x + (width - text.getWidth()) / 2, y + BOUNDARY_NAME_TOP));
            }
        }
        return rect;
    }

    /// The lines of a diagram: relationships first, then note anchors, which may end on them.
    private void edges(
        BasicDiagramEditor editor, String diagram, List<Map<String, Object>> presentations,
        Map<String, IPresentation> shown
    ) {
        for (boolean anchors : new boolean[] {false, true}) {
            for (Map<String, Object> p : presentations) {
                if ("edge".equals(p.get("shape")) && (p.get("element") == null) == anchors
                        && !shown.containsKey(p.get("id"))) {
                    edge(editor, diagram, p, shown);
                }
            }
        }
    }

    private void edge(
        BasicDiagramEditor editor, String diagram, Map<String, Object> p, Map<String, IPresentation> shown
    ) {
        IPresentation source = shown.get(p.get("source"));
        IPresentation target = shown.get(p.get("target"));
        if (source == null || target == null) {
            // An end that was not drawn: the line is lost with it.
            drop("presentation");
            return;
        }
        Object element = p.get("element");
        Object made = attempt(element == null ? (String) p.get("id") : (String) element, () -> {
            ILinkPresentation link;
            if (element == null) {
                // A note anchor runs from the note to what it annotates.
                if (isNote(source) && !isNote(target)) {
                    link = editor.createNoteAnchor((INodePresentation) source, target);
                } else if (isNote(target)) {
                    link = editor.createNoteAnchor((INodePresentation) target, source);
                } else {
                    drop("presentation");
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
            } else if (editor instanceof CompositeStructureDiagramEditor cs
                    && "connector".equals(type(elements.get(element)))
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = connector(cs, (String) element, s, t);
            } else if (created.get(element) instanceof IClass) {
                // A containment line stands for the nested class; the API cannot draw one.
                drop("containment");
                return null;
            } else if (editor instanceof ClassDiagramEditor cd && "link".equals(type(elements.get(element)))
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = link(cd, (String) element, s, t);
            } else if (editor instanceof StructureDiagramEditor sd && created.get(element) instanceof IElement model
                    && source instanceof INodePresentation s && target instanceof INodePresentation t) {
                link = sd.createLinkPresentation(model, s, t);
            } else {
                drop("presentation");
                return null;
            }
            lineDetails(link, source, target, diagram, p);
            return link;
        });
        if (made instanceof ILinkPresentation link) {
            shown.put((String) p.get("id"), link);
        }
    }

    /// The route, labels and looks of a line just created.
    private void lineDetails(ILinkPresentation link, IPresentation source, IPresentation target, String diagram,
            Map<String, Object> p) throws InvalidEditingException {
        // The line style goes first: Astah drops the waypoints of a line set to "line" and
        // turns them into corners of its own for right angles, but keeps points set later.
        String shape = lineShape(p);
        if (shape != null) {
            link.setProperty("line.shape", shape);
        }
        if (!route(link, source, target, p, corners(diagram, p))) {
            drop("edgeRoute");
        }
        labels(link, source, diagram, p);
        constraintLabels(link, diagram, p);
        if (p.get("style") instanceof Map<?, ?> s) {
            Map<String, Object> style = (Map<String, Object>) s;
            ink(link, style);
            if (Boolean.TRUE.equals(style.get("hideConstraints"))) {
                option(link, "constraint_visibility", "false");
            }
            // Astah keeps its shared style to itself (`is_shared` is read-only): the line keeps
            // its course over the common line, with a triangle of its own.
            if (style.get("shared") instanceof String shared && !"separate".equals(shared)) {
                drop("sharedGeneralization");
            }
        }
    }

    private static boolean isNote(IPresentation p) {
        return "Note".equals(p.getType());
    }

    /// Runs the line from the source anchor through the waypoints to the target anchor. Astah
    /// derives where the line meets a node from its first and last point, relative to the node.
    /// Returns false if Astah refused the line; it then keeps its own.
    private static boolean route(ILinkPresentation link, IPresentation source, IPresentation target,
            Map<String, Object> p, List<Object> waypoints) {
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

    /// Astah's `line.shape` for the edge's line style; null for straight lines, Astah's default.
    private static String lineShape(Map<String, Object> p) {
        Object shape = p.get("style") instanceof Map<?, ?> style ? style.get("lineShape") : null;
        if (shape == null) {
            return null;
        }
        return switch ((String) shape) {
            case "rightAngle" -> "line_right_angle";
            case "curve" -> "curve";
            case "curveRightAngle" -> "curve_right_angle";
            default -> null;
        };
    }

    /// The waypoints to export: for right-angled lines the corners as UmlCraft draws them (from
    /// the layout), otherwise the stored ones.
    private List<Object> corners(String diagram, Map<String, Object> p) {
        if (layouts.get(diagram) instanceof Map<?, ?> l && l.get("corners") instanceof Map<?, ?> all
                && all.get(p.get("id")) instanceof List<?> drawn) {
            return (List<Object>) drawn;
        }
        return list(p.get("waypoints"));
    }

    /// A point inside the node, given relative to its bounds; the center without an anchor.
    private static Point2D anchorPoint(INodePresentation node, Object anchor) {
        Map<String, Object> a = anchor instanceof Map<?, ?> m ? (Map<String, Object>) m : Map.of("x", 0.5, "y", 0.5);
        Point2D at = node.getLocation();
        return new Point2D.Double(at.getX() + num(a, "x") * node.getWidth(), at.getY() + num(a, "y") * node.getHeight());
    }

    /// Labels placed by hand. Astah keeps them per association end, which need not be the end
    /// the line starts at: the source labels belong to the end at the source node.
    private void labels(ILinkPresentation link, IPresentation sourceNode, String diagram, Map<String, Object> p)
            throws InvalidEditingException {
        Map<String, Object> l = (Map<String, Object>) layouts.get(diagram);
        if (l == null || !(l.get("labels") instanceof Map<?, ?> all)
                || !(all.get(p.get("id")) instanceof Map<?, ?> found)) {
            return;
        }
        Map<String, Object> placed = (Map<String, Object>) found;
        boolean swapped = link.getModel() instanceof IAssociation a
            && a.getMemberEnds()[0].getType() != sourceNode.getModel()
            && a.getMemberEnds()[1].getType() == sourceNode.getModel();
        String source = swapped ? "end_b" : "end_a";
        String target = swapped ? "end_a" : "end_b";
        label(link, "name", placed.get("name"));
        label(link, source + ".name", placed.get("sourceRole"));
        label(link, source + ".multiplicity", placed.get("sourceMultiplicity"));
        label(link, target + ".name", placed.get("targetRole"));
        label(link, target + ".multiplicity", placed.get("targetMultiplicity"));
    }

    /// Constraint labels moved away from where Astah puts them. Astah moves a line's constraint
    /// labels together: `constraint.point` places the first one, and the others move by as
    /// much. Only associations take it, so the places go only where all labels moved by the
    /// same offset on an association; elsewhere they are dropped. The offset is added to where
    /// Astah has put the first label, so Astah's own text widths center it.
    private void constraintLabels(ILinkPresentation link, String diagram, Map<String, Object> p) {
        Map<String, Object> l = (Map<String, Object>) layouts.get(diagram);
        if (l == null || !(l.get("constraints") instanceof Map<?, ?> all)
                || !(all.get(p.get("id")) instanceof Map<?, ?> found)) {
            return;
        }
        try {
            if (found.get("together") instanceof Map<?, ?> by && link.getModel() instanceof IAssociation) {
                Map<String, Object> offset = (Map<String, Object>) by;
                String x = link.getProperty("constraint.point.x");
                String y = link.getProperty("constraint.point.y");
                if (x != null && y != null) {
                    link.setProperty("constraint.point.x", Double.toString(Double.parseDouble(x) + num(offset, "x")));
                    link.setProperty("constraint.point.y", Double.toString(Double.parseDouble(y) + num(offset, "y")));
                    return;
                }
            }
        } catch (InvalidEditingException | NumberFormatException x) {
            // Dropped below.
        }
        drop("constraintPosition");
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
        boolean iconic = style != null && Boolean.TRUE.equals(style.get("iconic"));
        if ("interface".equals(type)) {
            node.setProperty("notation_type", iconic ? "icon" : "normal");
            // Astah draws a socket only for a port's usage (`portInterfaces`), not here.
            if (iconic && Boolean.TRUE.equals(style.get("socket"))) {
                drop("socket");
            }
        }
        // A class whose first stereotype is «boundary», «control» or «entity» starts out as
        // its robustness icon in Astah; UmlCraft says which.
        if ("class".equals(type) && robustness((String) p.get("element"))) {
            node.setProperty("notation_type", iconic ? "icon" : "normal");
        }
        // Without a color of its own a node gets Astah's usual default, not the API's white.
        String fill = ownFill(style);
        node.setProperty("fill.color", fill != null ? fill : DEFAULT_FILL);
        if (style == null) {
            return;
        }
        if (Boolean.TRUE.equals(style.get("hideAttributes"))) {
            // An object's compartment holds its slots.
            node.setProperty("instance".equals(type) ? "slot_visibility" : "attribute_compartment_visibility", "false");
        }
        if ("instance".equals(type) && Boolean.TRUE.equals(style.get("hideEmptySlots"))) {
            node.setProperty("no_value_slot_visibility", "false");
        }
        // The API keeps neither an instance's hidden name nor its hidden classifier.
        if (Boolean.TRUE.equals(style.get("hideInstanceName"))) {
            drop("instanceNameVisibility");
        }
        if (Boolean.TRUE.equals(style.get("hideInstanceType"))) {
            drop("instanceTypeVisibility");
        }
        if (Boolean.TRUE.equals(style.get("hideOperations"))) {
            node.setProperty("operation_compartment_visibility", "false");
        }
        if (Boolean.TRUE.equals(style.get("hideAttributeConstraints"))) {
            option(node, "attribute_constraint_visibility", "false");
        }
        if (Boolean.TRUE.equals(style.get("hideOperationConstraints"))) {
            option(node, "operation_constraint_visibility", "false");
        }
        // The API refuses every value of `font.size`.
        if (style.get("fontSize") != null) {
            drop("fontSize");
        }
        ink(node, style);
        if (Boolean.TRUE.equals(style.get("hideStereotypes"))) {
            option(node, "stereotype_visibility", "false");
        }
        if ("class".equals(type) || "interface".equals(type) || "enumeration".equals(type)
                || "dataType".equals(type)) {
            members(node, style, (String) p.get("element"));
        }
    }

    /// Whether the element's first stereotype is one of the robustness stereotypes.
    private boolean robustness(String element) {
        List<Object> stereotypes = list(elements.get(element) == null ? null : elements.get(element).get("stereotypes"));
        return !stereotypes.isEmpty()
            && Set.of("boundary", "control", "entity").contains(((String) stereotypes.get(0)).toLowerCase());
    }

    /// What a classifier box leaves out of its members, under the names of Astah's API.
    private void members(INodePresentation node, Map<String, Object> style, String element) {
        String[][] switches = {
            {"hideMemberVisibility", "visibilitye_kind_visibility"},
            {"hideAttributeTypes", "attribute_type_visibility"},
            {"hideInitialValues", "attribute_initial_value_visibility"},
            {"hideAttributeStereotypes", "attribute_stereotype_visibility"},
            {"hideOperationStereotypes", "operation_stereotype_visibility"},
            {"hideParameters", "operation_parameter_visibility"},
            {"hideParameterTypes", "operation_parameter_type_visibility"},
            {"hideReturnTypes", "operation_return_type_visibility"},
        };
        for (String[] s : switches) {
            if (Boolean.TRUE.equals(style.get(s[0]))) {
                option(node, s[1], "false");
            }
        }
        // Astah shows parameter directions all or none; UmlCraft by default shows `out` and
        // `inout`, so a box with such a parameter shows them all in Astah.
        boolean directions = switch (style.get("parameterDirections") instanceof String d ? d : "explicit") {
            case "all" -> true;
            case "hidden" -> false;
            default -> hasDirections(element);
        };
        if (directions) {
            option(node, "operation_parameter_direction_kind_visibility", "true");
        }
        for (String[] kind : new String[][] {{"hiddenAttributes", "attribute"}, {"hiddenOperations", "operation"}}) {
            for (Object v : list(style.get(kind[0]))) {
                option(node, v + kind[1], "false");
            }
        }
    }

    /// Whether an operation of the classifier has an `out` or `inout` parameter.
    private boolean hasDirections(String classifier) {
        for (String member : children(classifier)) {
            for (String parameter : children(member)) {
                String direction = text(elements.get(parameter), "direction");
                if ("out".equals(direction) || "inOut".equals(direction)) {
                    return true;
                }
            }
        }
        return false;
    }

    /// The text and line colors of a shape or line other than Astah's black.
    private void ink(IPresentation presentation, Map<String, Object> style) {
        for (String[] key : new String[][] {{"fontColor", "font.color"}, {"stroke", "line.color"}}) {
            if (style.get(key[0]) instanceof String c && c.length() >= 7) {
                option(presentation, key[1], c.substring(0, 7).toUpperCase());
            }
        }
    }

    /// A display option; one the API refuses for this kind of shape is reported.
    private void option(IPresentation presentation, String key, String value) {
        try {
            presentation.setProperty(key, value);
        } catch (InvalidEditingException e) {
            drop("displayOption");
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
                    String fill = ownFill(style);
                    bars.add(new Bar((String) p.get("lifeline"), top, top + num(p, "height"),
                        fill != null ? fill : DEFAULT_FILL));
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
                // As for instances, the API keeps neither a hidden name nor a hidden classifier.
                if (p.get("style") instanceof Map<?, ?> style) {
                    if (Boolean.TRUE.equals(style.get("hideInstanceName"))) {
                        drop("instanceNameVisibility");
                    }
                    if (Boolean.TRUE.equals(style.get("hideInstanceType"))) {
                        drop("instanceTypeVisibility");
                    }
                }
            }

            walk(owners.get(diagram), null);
            for (Fragment f : fragments) {
                attempt(f.id(), () -> fragment(f));
            }
            displayAll();

            // Astah grows bars itself, mostly as UmlCraft does; what it can be told is kept.
            // Astah refuses to set any bar's height, so only a bar it made differently counts.
            for (Bar bar : bars) {
                if (bar.astah != null && Math.abs(bar.astah.getHeight() - (bar.bottom - bar.top)) > 0.5) {
                    try {
                        bar.astah.setHeight(bar.bottom - bar.top);
                    } catch (InvalidEditingException e) {
                        drop("barHeight");
                    }
                }
            }
            lengths(true);
            // State invariants, once the lifelines have their length, then the terminations,
            // which Astah puts where their lifeline ends. Astah fits the lifelines anew after
            // each (15 below an invariant), so the lengths are set again.
            for (String kind : new String[] {"stateInvariant", "termination"}) {
                for (Map<String, Object> p : presentations) {
                    if (p.get("element") instanceof String element && "node".equals(p.get("shape"))
                            && kind.equals(type(elements.get(element)))) {
                        attempt(element, () -> mark(element, elements.get(element), p));
                    }
                }
                lengths(false);
            }
            // Astah's API has no way to make duration or time constraints.
            for (Map<String, Object> p : presentations) {
                String type = p.get("element") instanceof String element ? type(elements.get(element)) : "";
                if ("node".equals(p.get("shape"))
                        && ("durationConstraint".equals(type) || "timeConstraint".equals(type))) {
                    drop(type);
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

        /// Gives every lifeline its length; `report` counts those Astah refuses.
        private void lengths(boolean report) {
            for (Map.Entry<String, INodePresentation> lane : lanes.entrySet()) {
                double length = num(laneShapes.get(lane.getKey()), "length");
                try {
                    lane.getValue().setProperty("lifeline_length", Double.toString(length));
                } catch (InvalidEditingException e) {
                    // Astah keeps lifelines long enough for what lies on them.
                    if (report) {
                        drop("lifelineLength");
                    }
                }
            }
        }

        /// The Astah lifeline showing the UmlCraft lifeline element `lifeline`.
        private INodePresentation laneOf(Object lifeline) {
            for (Map.Entry<String, Map<String, Object>> lane : laneShapes.entrySet()) {
                if (lifeline != null && lifeline.equals(lane.getValue().get("element"))) {
                    return lanes.get(lane.getKey());
                }
            }
            return null;
        }

        /// A state invariant where UmlCraft draws it, or a termination: Astah puts its cross
        /// where the lifeline ends, as UmlCraft does.
        private INodePresentation mark(String id, Map<String, Object> e, Map<String, Object> p) throws Exception {
            INodePresentation lane = laneOf(e.get("lifeline"));
            if (lane == null) {
                drop(type(e));
                return null;
            }
            if ("termination".equals(type(e))) {
                return editor.createTermination(lane);
            }
            // Astah puts its top at the height given, centered on the lifeline, and refuses
            // to move it elsewhere. Its width is fixed; its height follows the text.
            Map<String, Object> r = rect(diagram, p);
            INodePresentation node = editor.createStateInvariant(lane, name(e), y(num(r, "y")));
            common(id, node.getModel(), Set.of());
            return node;
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
            // A call without arguments shows its operation's parameters, with their types and
            // the return type as chosen.
            if (link.getModel() instanceof IMessage message && !message.isReturnMessage()) {
                boolean hideTypes = style != null
                        && Boolean.TRUE.equals(style.get("hideParameterTypes"));
                boolean showReturn = style != null
                        && Boolean.TRUE.equals(style.get("showReturnType"));
                link.setProperty("parameter_type_visibility", String.valueOf(!hideTypes));
                link.setProperty("return_value_visibility", String.valueOf(showReturn));
            }
            Map<String, Object> labels = (Map<String, Object>) p.get("labels");
            if (labels != null && labels.get("name") instanceof Map<?, ?> offset) {
                Map<String, Object> head = head(laneShapes.get(from));
                double center = num(head, "x") + num(head, "width") / 2;
                double x = center + num((Map<String, Object>) offset, "x");
                double y = y(at + num((Map<String, Object>) offset, "y"));
                Object defaults = layouts.get(diagram) instanceof Map<?, ?> l ? l.get("labelDefaults") : null;
                Object own = defaults instanceof Map<?, ?> d ? d.get(p.get("id")) : null;
                // Found and lost messages may have no label position of their own yet.
                String current = link.getProperty("name.point.x");
                double shift = own instanceof Number n && current != null
                    ? Double.parseDouble(current) - n.doubleValue() : LABEL_SHIFT;
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
                double gx = p.get("gateX") instanceof Number n ? n.doubleValue() : x - GATE_DISTANCE;
                link = editor.createFoundMessage(name(e), new Point2D.Double(gx, y(at)), lifeline);
                keep(link.getTarget(), lane, at);
            } else {
                double gx = p.get("gateX") instanceof Number n ? n.doubleValue() : x + GATE_DISTANCE;
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
                // Without a guard text there is no label to place.
                String guard = made.get(k).guard();
                if (guard != null && !guard.isEmpty()
                        && i < guards.size() && guards.get(i) instanceof Map<?, ?> moved) {
                    double operandTop = i == 0 ? top + FIRST_OPERAND_TOP
                        : top + ((Number) separators.get(i - 1)).doubleValue();
                    double x = num(r, "x") + 10 + num((Map<String, Object>) moved, "x");
                    double gy = y(operandTop + 2 + num((Map<String, Object>) moved, "y"));
                    try {
                        node.setProperty("guard." + (k + 1) + ".point.x", Double.toString(x));
                        node.setProperty("guard." + (k + 1) + ".point.y", Double.toString(gy));
                    } catch (InvalidEditingException refused) {
                        // Astah refuses guards moved above their operand.
                        drop("guardPosition");
                    }
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
                problems.add(oneLine(what + name + ": " + describe(e)));
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

    /// UmlCraft's names of the language profiles' tagged values and Astah's.
    private static final String[][] LANGUAGE_TAGS = {
        {"java.", "jude.java."}, {"csharp.", "jude.c_sharp."}, {"cpp.", "jude.c_plus."},
    };

    /// Astah's key for a tagged value: `java.transient` is `jude.java.transient` there.
    private static String astahTag(String key) {
        for (String[] names : LANGUAGE_TAGS) {
            if (key.startsWith(names[0])) {
                return names[1] + key.substring(names[0].length());
            }
        }
        return key;
    }

    /// Visibility, stereotypes, tagged values and documentation.
    private void common(String id, IElement target, Set<String> implied) {
        Map<String, Object> e = elements.get(id);
        if (e == null || target == null) {
            return;
        }
        try {
            // UmlCraft leaves out the default, public; Astah's default for attributes is private.
            if (target instanceof INamedElement n && e.get("visibility") instanceof String v) {
                n.setVisibility(v);
            } else if (target instanceof INamedElement n && !(target instanceof IModel)) {
                try {
                    n.setVisibility("public");
                } catch (InvalidEditingException x) {
                    // Template parameters, for one, have no visibility of their own.
                }
            }
            for (Object s : list(e.get("stereotypes"))) {
                if (!implied.contains(s) && !target.hasStereotype((String) s)) {
                    target.addStereotype((String) s);
                }
            }
            for (Object tv : list(e.get("taggedValues"))) {
                Map<String, Object> tag = (Map<String, Object>) tv;
                String key = text(tag, "key");
                try {
                    basic.createTaggedValue(target, astahTag(key), text(tag, "value"));
                } catch (InvalidEditingException x) {
                    // Astah keeps keys such as "jude.java.annotations" to itself; UmlCraft's
                    // own name keeps the value.
                    try {
                        basic.createTaggedValue(target, key, text(tag, "value"));
                    } catch (InvalidEditingException y) {
                        drop("details");
                    }
                }
            }
            if (target instanceof INamedElement n && !text(e, "documentation").isEmpty()) {
                n.setDefinition(text(e, "documentation"));
            }
        } catch (InvalidEditingException x) {
            drop("details");
            if (problems.size() < 20) {
                problems.add(oneLine(type(e) + " \"" + name(e) + "\": " + describe(x)));
            }
        }
        constraints(id, target);
    }

    /// The element's constraints; Astah names each by its text and keeps the text as its
    /// specification.
    private void constraints(String id, IElement target) {
        Map<String, Object> e = elements.get(id);
        for (Object c : list(e.get("constraints"))) {
            try {
                if (!(target instanceof INamedElement n)) {
                    throw new IllegalStateException("no named element");
                }
                basic.createConstraint(n, (String) c);
            } catch (Exception x) {
                drop("constraint");
                if (problems.size() < 20) {
                    problems.add(oneLine(type(e) + " \"" + name(e) + "\" constraint: " + describe(x)));
                }
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
                if (i >= s.length()) {
                    throw error("unterminated string");
                }
                char escape = s.charAt(i++);
                switch (escape) {
                    case 'n' -> out.append('\n');
                    case 't' -> out.append('\t');
                    case 'r' -> out.append('\r');
                    case 'b' -> out.append('\b');
                    case 'f' -> out.append('\f');
                    case 'u' -> {
                        if (i + 4 > s.length()) {
                            throw error("unterminated string");
                        }
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
