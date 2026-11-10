//! Why an edit the user asked for was refused, as `ModelError::Invalid` carries it. The texts
//! are English for AI clients and logs; the app shows each in the user's language by matching
//! these constants (`umlcraft-app`, `errors::model_error`). A text with a varying part names
//! the fixed part it starts or ends with.

/// The element is shown on the diagram already; a diagram shows it once.
pub const ALREADY_SHOWN: &str = "the element is already on this diagram";
/// A lifeline or fragment of another interaction dropped on a sequence diagram.
pub const OTHER_INTERACTION: &str = "the element belongs to another interaction";
/// Ends `a <kind> cannot be shown on this diagram`.
pub const CANNOT_SHOW: &str = " cannot be shown on this diagram";
/// In `<tool> cannot connect <kind> to <kind>`.
pub const CANNOT_CONNECT: &str = " cannot connect ";
/// Starts `cannot paste a <kind> here`.
pub const CANNOT_PASTE: &str = "cannot paste a ";
/// A connector from a part or port to itself.
pub const CONNECTOR_SAME_ENDS: &str = "a connector needs two different ends";
/// A connector between parts of different classes.
pub const CONNECTOR_NO_CLASS: &str = "no class holds both ends of the connector";
/// Starts `a connector cannot connect a <kind>`.
pub const CONNECTOR_CANNOT: &str = "a connector cannot connect a ";
/// A second create message to one lifeline.
pub const CREATED_ALREADY: &str = "the lifeline is created already";
/// No height between the lifelines' ends where the message fits.
pub const NO_ROOM: &str = "no room for the message on these lifelines";
/// A message drawn to something that is no lifeline.
pub const MESSAGES_CONNECT_LIFELINES: &str = "messages connect lifelines";
/// Arranging an activity whose partitions lie one below the other.
pub const PARTITIONS_STACKED: &str = "partitions one below the other";
/// Arranging a sequence diagram.
pub const SEQUENCE_NOT_ARRANGED: &str = "sequence diagrams are not arranged";
/// An element renamed to nothing.
pub const EMPTY_NAME: &str = "the name must not be empty";
/// A bound class whose binding names no template.
pub const BINDING_WITHOUT_TEMPLATE: &str = "a binding names a template and its arguments";
/// Converting an association that has an association class already.
pub const HAS_ASSOCIATION_CLASS: &str = "the association has an association class";
/// Starts `no valid name in "<text>"`, for a member typed without one.
pub const MEMBER_NO_NAME: &str = "no valid name in ";
/// Starts `invalid multiplicity "<text>"`.
pub const MEMBER_MULTIPLICITY: &str = "invalid multiplicity ";
/// Starts `missing "<char>" in "<text>"`, for an unclosed bracket or quote.
pub const MEMBER_MISSING: &str = "missing \"";
/// Starts `unexpected "<text>" after "<char>"`.
pub const MEMBER_UNEXPECTED: &str = "unexpected ";
