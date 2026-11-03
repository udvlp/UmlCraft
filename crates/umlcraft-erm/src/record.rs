//! The `.ucerm` file as serde sees it: flat records with every field any kind of node may
//! have. Reading turns them into the typed model and rejects fields that do not belong to a
//! node's kind; writing leaves out what has its default value.

use serde::{Deserialize, Serialize, Serializer};

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

pub(crate) fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Numbers without a fraction are written without `.0` (`80`, not `80.0`), as draw.io and
/// people write coordinates.
pub(crate) fn num<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(*v)
    }
}

pub(crate) fn opt_num<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(v) => num(v, s),
        None => s.serialize_none(),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FileRecord {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub meta: MetaRecord,
    #[serde(default)]
    pub nodes: Vec<NodeRecord>,
    #[serde(default)]
    pub links: Vec<LinkRecord>,
    #[serde(default)]
    pub texts: Vec<TextRecord>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MetaRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notation: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub(crate) struct RectRecord {
    #[serde(serialize_with = "num")]
    pub x: f64,
    #[serde(serialize_with = "num")]
    pub y: f64,
    #[serde(serialize_with = "num")]
    pub width: f64,
    #[serde(serialize_with = "num")]
    pub height: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub(crate) struct PointRecord {
    #[serde(serialize_with = "num")]
    pub x: f64,
    #[serde(serialize_with = "num")]
    pub y: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NodeRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    pub bounds: RectRecord,
    #[serde(default, skip_serializing_if = "is_false")]
    pub weak: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub derived: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub multivalued: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub identifying: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub total: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disjoint: Option<bool>,
    #[serde(default, skip_serializing_if = "StyleRecord::is_default")]
    pub style: StyleRecord,
    #[serde(default, skip_serializing_if = "PropsRecord::is_default")]
    pub props: PropsRecord,
}

#[derive(Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StyleRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", serialize_with = "opt_num")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub rounded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
}

impl StyleRecord {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PropsRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_precision: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub not_null: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl PropsRecord {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinkRecord {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_port: Option<PointRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_port: Option<PointRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<LabelRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<LabelRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub super_type: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<PointRecord>,
}

/// A count or `"n"`.
/// A number, or a word: `max` `"n"`/`"m"`, `min` `"*"`.
#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum MaxRecord {
    Count(u32),
    Word(String),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LabelRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<MaxRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<MaxRecord>,
    #[serde(default, skip_serializing_if = "is_zero", serialize_with = "num")]
    pub x: f64,
    #[serde(default, skip_serializing_if = "is_zero", serialize_with = "num")]
    pub y: f64,
    #[serde(default, skip_serializing_if = "is_zero", serialize_with = "num")]
    pub dx: f64,
    #[serde(default, skip_serializing_if = "is_zero", serialize_with = "num")]
    pub dy: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TextRecord {
    pub id: String,
    pub bounds: RectRecord,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "StyleRecord::is_default")]
    pub style: StyleRecord,
}
