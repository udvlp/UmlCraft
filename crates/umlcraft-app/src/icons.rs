//! The app's icons: `assets/icons/` embedded in the binary, and which icon stands for a tool, a
//! diagram kind or a model element. Rules for the files are in `assets/icons/README.md`.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};
use umlcraft_model::{
    ActivityNodeKind, DiagramKind, ElementKind, InstanceKind, MessageSort, PseudostateKind,
};
use umlcraft_notation::palette::{EdgeTool, NodeTool, Tool};

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets"]
#[include = "icons/**/*.svg"]
#[include = "logo.svg"]
struct Embedded;

/// UmlCraft's icons first, then the Lucide icons gpui-component draws itself (chevrons, checks).
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Embedded::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> =
            Embedded::iter().filter(|name| name.starts_with(path)).map(Into::into).collect();
        names.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(names)
    }
}

/// The app's logo, in its own colors.
pub const LOGO: &str = "logo.svg";

/// A Fluent icon by its name, e.g. `fluent("save")` for `ic_fluent_save_24_regular.svg`.
pub fn fluent(name: &str) -> SharedString {
    format!("icons/fluent/ic_fluent_{name}_24_regular.svg").into()
}

fn uml(name: &str) -> SharedString {
    format!("icons/uml/{name}.svg").into()
}

pub fn tool_icon(tool: Tool) -> SharedString {
    match tool {
        Tool::Select => fluent("cursor"),
        Tool::Text => fluent("text_t"),
        Tool::Line => uml("freeLine"),
        Tool::Node(_) | Tool::Edge(_) => uml(tool.key()),
    }
}

/// A diagram kind shows the icon of the element it is mostly about.
pub fn diagram_icon(kind: DiagramKind) -> SharedString {
    tool_icon(Tool::Node(match kind {
        DiagramKind::Class => NodeTool::Class,
        DiagramKind::UseCase => NodeTool::UseCase,
        DiagramKind::Sequence => NodeTool::Lifeline,
        DiagramKind::Activity => NodeTool::Action,
        DiagramKind::StateMachine => NodeTool::State,
        DiagramKind::Component => NodeTool::Component,
        DiagramKind::Deployment => NodeTool::DeploymentNode,
    }))
}

/// Structograms, which have no tool of their own.
pub fn structogram_icon() -> SharedString {
    uml("structogram")
}

/// A kind of structogram element, as Structorizer draws it.
pub fn nsd_kind_icon(kind: umlcraft_nsd::ElementKind) -> SharedString {
    let tag = kind.tag();
    uml(&format!("nsd{}{}", tag[..1].to_uppercase(), &tag[1..]))
}

/// A command on structogram elements: `color`, `disabled`, `addBranch`, `removeBranch`.
/// One of UmlCraft's own icons that stands for no tool, e.g. `messageNumbers`.
pub fn own_icon(name: &str) -> SharedString {
    uml(name)
}

pub fn nsd_icon(name: &str) -> SharedString {
    uml(&format!("nsd{}{}", name[..1].to_uppercase(), &name[1..]))
}

/// The icon in front of a model element in the tree and the properties panel; members and other
/// elements without a notation of their own have none.
pub fn kind_icon(kind: &ElementKind) -> Option<SharedString> {
    use EdgeTool as E;
    use NodeTool as N;
    let tool = |t: Tool| Some(tool_icon(t));
    let node = |n: NodeTool| tool(Tool::Node(n));
    let edge = |e: EdgeTool| tool(Tool::Edge(e));
    match kind {
        ElementKind::Package => node(N::Package),
        ElementKind::Class(_) | ElementKind::DataType(_) => node(N::Class),
        ElementKind::Interface(_) => node(N::Interface),
        ElementKind::Enumeration(_) => node(N::Enumeration),
        ElementKind::Actor(_) => node(N::Actor),
        ElementKind::UseCase(_) => node(N::UseCase),
        ElementKind::Component(_) => node(N::Component),
        ElementKind::Artifact(_) => node(N::Artifact),
        ElementKind::Node(_) => node(N::DeploymentNode),
        ElementKind::Instance(p) => node(match p.instance {
            InstanceKind::Object => N::Object,
            InstanceKind::Component => N::ComponentInstance,
            InstanceKind::Node => N::NodeInstance,
        }),
        ElementKind::SystemBoundary => node(N::SystemBoundary),
        ElementKind::Lifeline(_) => node(N::Lifeline),
        ElementKind::Message(p) => edge(match p.sort {
            MessageSort::Synchronous => E::Message,
            MessageSort::Asynchronous => E::AsyncMessage,
            MessageSort::Reply => E::Reply,
            MessageSort::Create => E::CreateMessage,
            MessageSort::Destroy => E::DestroyMessage,
        }),
        ElementKind::CombinedFragment(_) => node(N::CombinedFragment),
        ElementKind::InteractionUse(_) => node(N::InteractionUse),
        ElementKind::ActivityNode(p) => node(match p.node {
            ActivityNodeKind::Action => N::Action,
            ActivityNodeKind::Initial => N::InitialNode,
            ActivityNodeKind::ActivityFinal => N::ActivityFinal,
            ActivityNodeKind::FlowFinal => N::FlowFinal,
            ActivityNodeKind::Decision => N::Decision,
            ActivityNodeKind::Merge => N::Merge,
            ActivityNodeKind::Fork => N::Fork,
            ActivityNodeKind::Join => N::Join,
            ActivityNodeKind::Object => N::ObjectNode,
        }),
        ElementKind::ActivityEdge(_) => edge(E::Flow),
        ElementKind::Partition => node(N::Partition),
        ElementKind::State(_) => node(N::State),
        ElementKind::FinalState => node(N::FinalState),
        ElementKind::Pseudostate(p) => node(match p.pseudostate {
            PseudostateKind::Initial => N::InitialPseudostate,
            PseudostateKind::Choice => N::Choice,
            PseudostateKind::Junction => N::Junction,
            PseudostateKind::Fork => N::ForkPseudostate,
            PseudostateKind::Join => N::JoinPseudostate,
            PseudostateKind::ShallowHistory => N::ShallowHistory,
            PseudostateKind::DeepHistory => N::DeepHistory,
        }),
        ElementKind::Transition(_) | ElementKind::InternalTransition(_) => edge(E::Transition),
        ElementKind::Association(_) => edge(E::Association),
        ElementKind::Link(_) => edge(E::Link),
        ElementKind::Generalization(_) => edge(E::Generalization),
        ElementKind::Realization(_) => edge(E::Realization),
        ElementKind::Dependency(_) => edge(E::Dependency),
        ElementKind::Include(_) => edge(E::Include),
        ElementKind::Extend(_) => edge(E::Extend),
        ElementKind::Comment(_) => node(N::Note),
        ElementKind::Diagram(p) => Some(diagram_icon(p.diagram)),
        ElementKind::Structogram(_) => Some(structogram_icon()),
        ElementKind::Model
        | ElementKind::EnumerationLiteral
        | ElementKind::Attribute(_)
        | ElementKind::Operation(_)
        | ElementKind::Parameter(_)
        | ElementKind::TemplateParameter(_)
        | ElementKind::ExtensionPoint
        | ElementKind::Interaction
        | ElementKind::InteractionOperand(_)
        | ElementKind::Activity
        | ElementKind::StateMachine
        | ElementKind::Region
        | ElementKind::Slot(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [DiagramKind; 7] = [
        DiagramKind::Class,
        DiagramKind::UseCase,
        DiagramKind::Sequence,
        DiagramKind::Activity,
        DiagramKind::StateMachine,
        DiagramKind::Component,
        DiagramKind::Deployment,
    ];

    #[test]
    fn every_tool_and_diagram_kind_has_an_embedded_icon() {
        for kind in KINDS {
            let icon = diagram_icon(kind);
            assert!(Embedded::get(&icon).is_some(), "{icon}");
            for tool in umlcraft_notation::palette::palette(kind) {
                let icon = tool_icon(tool);
                assert!(Embedded::get(&icon).is_some(), "{tool:?}: {icon}");
            }
        }
        assert!(Embedded::get(&structogram_icon()).is_some());
        for kind in umlcraft_nsd::ElementKind::ALL {
            let icon = nsd_kind_icon(kind);
            assert!(Embedded::get(&icon).is_some(), "{icon}");
        }
        for name in ["color", "disabled", "addBranch", "removeBranch"] {
            assert!(Embedded::get(&nsd_icon(name)).is_some(), "{name}");
        }
        assert!(Embedded::get(LOGO).is_some());
    }

    #[test]
    fn other_icons_come_from_gpui_kit() {
        assert!(AppAssets.load("icons/chevron-down.svg").unwrap().is_some());
    }
}
