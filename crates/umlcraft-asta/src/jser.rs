//! Reader and writer for the Java Object Serialization Stream Protocol.
//!
//! Astah stores its model as one serialized object graph. This module knows nothing about
//! Astah; it turns the stream into a [`Graph`] of generic objects that the importer then reads
//! by class and field name, and [`write`] turns a graph back into a stream.
//!
//! Data written by custom `writeObject` methods ends up in [`ClassData::annotation`] as raw
//! block data and embedded objects; [`Annotation`] reads it back in order.
//!
//! The graph keeps everything the stream says, so writing a graph that was read gives the same
//! bytes again: class descriptions with their serial version UIDs and field type names, the
//! chunks of block data, the identity of objects and stream resets.

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

mod write;

pub use write::write;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("unexpected end of stream at byte {0}")]
    Eof(usize),
    #[error("not a Java serialization stream")]
    BadMagic,
    #[error("unexpected type code 0x{code:02x} at byte {at}")]
    BadTypeCode { code: u8, at: usize },
    #[error("unknown handle 0x{0:x}")]
    BadHandle(u32),
    #[error("handle 0x{0:x} is not a {1}")]
    WrongHandle(u32, &'static str),
    #[error("invalid field type {0:?}")]
    BadFieldType(char),
    #[error("externalizable class {0} uses the old protocol and cannot be read")]
    Externalizable(String),
    #[error("object graph nested too deeply")]
    TooDeep,
    #[error("class {0} is its own superclass")]
    ClassCycle(String),
    #[error("class {0} has too many superclasses")]
    ClassChainTooLong(String),
    #[error("cannot write {0}")]
    Unwritable(&'static str),
    #[error("the stream decodes to more data than it can hold")]
    TooLarge,
}

type Result<T> = std::result::Result<T, StreamError>;

/// The element type of an array class (`[I`, `[Ljava.lang.String;`). A class that is no array
/// is a broken stream, not an array of objects.
pub(crate) fn array_element(class_name: &str) -> Result<FieldType> {
    match class_name.as_bytes() {
        [b'[', code, ..] => FieldType::from_code(*code),
        _ => Err(StreamError::BadFieldType(class_name.chars().next().unwrap_or('?'))),
    }
}

/// Index of an object in [`Graph::objects`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjId(pub usize);

/// Index of a class description in [`Graph::classes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassId(pub usize);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Byte(i8),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Object(ObjId),
}

impl Value {
    pub fn as_object(&self) -> Option<ObjId> {
        match *self {
            Self::Object(id) => Some(id),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// Any integral value widened to `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Byte(v) => Some(v.into()),
            Self::Char(v) => Some(v.into()),
            Self::Short(v) => Some(v.into()),
            Self::Int(v) => Some(v.into()),
            Self::Long(v) => Some(v),
            _ => None,
        }
    }

    /// Any numeric value as `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float(v) => Some(v.into()),
            Self::Double(v) => Some(v),
            _ => self.as_i64().map(|v| v as f64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldType {
    Byte,
    Char,
    Double,
    Float,
    Int,
    Long,
    Short,
    Bool,
    Object,
}

impl FieldType {
    pub(crate) fn from_code(code: u8) -> Result<Self> {
        Ok(match code {
            b'B' => Self::Byte,
            b'C' => Self::Char,
            b'D' => Self::Double,
            b'F' => Self::Float,
            b'I' => Self::Int,
            b'J' => Self::Long,
            b'S' => Self::Short,
            b'Z' => Self::Bool,
            b'L' | b'[' => Self::Object,
            other => return Err(StreamError::BadFieldType(other as char)),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ClassDesc {
    /// Shared with the [`ClassData`] of every instance, which names its class by it.
    pub name: Arc<str>,
    pub serial_version_uid: i64,
    /// `SC_*` flags.
    pub flags: u8,
    pub fields: Vec<FieldDesc>,
    /// What the stream's `annotateClass` wrote; empty for a plain `ObjectOutputStream`.
    pub annotation: Vec<Content>,
    pub super_class: Option<ClassId>,
    /// The interfaces of a dynamic proxy class (whose name is `$Proxy`).
    pub interfaces: Option<Vec<String>>,
}

impl ClassDesc {
    /// A description without fields, annotation or superclass.
    pub fn new(name: impl Into<Arc<str>>, serial_version_uid: i64, flags: u8) -> Self {
        Self {
            name: name.into(),
            serial_version_uid,
            flags,
            fields: Vec::new(),
            annotation: Vec::new(),
            super_class: None,
            interfaces: None,
        }
    }
}

/// A serialized field of a class description.
#[derive(Clone, Debug)]
pub struct FieldDesc {
    /// Shared with every instance's [`ClassData`], which names its values by it.
    pub name: Arc<str>,
    /// `B`, `C`, `D`, `F`, `I`, `J`, `S`, `Z`, or `L` and `[` for objects and arrays.
    pub code: u8,
    /// The field's type in JVM notation (`Ljava/lang/String;`), a string object; `None` for
    /// primitive fields.
    pub type_name: Option<Value>,
}

impl FieldDesc {
    pub(crate) fn ty(&self) -> FieldType {
        FieldType::from_code(self.code).unwrap_or(FieldType::Object)
    }
}

pub const SC_WRITE_METHOD: u8 = 0x01;
pub const SC_SERIALIZABLE: u8 = 0x02;
pub const SC_EXTERNALIZABLE: u8 = 0x04;
pub const SC_BLOCK_DATA: u8 = 0x08;
pub const SC_ENUM: u8 = 0x10;

/// The serialized state one class of an object's hierarchy contributed.
#[derive(Clone, Debug, Default)]
pub struct ClassData {
    pub class: Arc<str>,
    pub fields: Vec<(Arc<str>, Value)>,
    /// What a custom `writeObject` wrote after the default fields.
    pub annotation: Vec<Content>,
}

/// One item of an annotation or of the stream: raw block data, an object (possibly `null`) or,
/// at the top level only, a reset of the stream's handles.
#[derive(Clone, Debug)]
pub enum Content {
    Block(Vec<u8>),
    Value(Value),
    Reset,
}

#[derive(Clone, Debug)]
pub enum Object {
    String(String),
    /// Class data from the topmost superclass down to the object's own class.
    Instance {
        class: ClassId,
        data: Vec<ClassData>,
    },
    Array {
        class: ClassId,
        items: Vec<Value>,
    },
    Enum {
        class: ClassId,
        constant: String,
        /// The string object that carried the constant's name.
        name: Value,
    },
    Class(ClassId),
}

#[derive(Debug, Default)]
pub struct Graph {
    pub classes: Vec<ClassDesc>,
    pub objects: Vec<Object>,
    /// Top-level contents of the stream, in order.
    pub roots: Vec<Content>,
}

impl Graph {
    pub fn object(&self, id: ObjId) -> &Object {
        &self.objects[id.0]
    }

    pub fn class(&self, id: ClassId) -> &ClassDesc {
        &self.classes[id.0]
    }

    /// Class name of an instance, array, enum constant or class object; `None` for strings.
    pub fn class_name(&self, id: ObjId) -> Option<&str> {
        match self.object(id) {
            Object::Instance { class, .. }
            | Object::Array { class, .. }
            | Object::Enum { class, .. }
            | Object::Class(class) => Some(&self.class(*class).name),
            Object::String(_) => None,
        }
    }

    pub fn string(&self, id: ObjId) -> Option<&str> {
        match self.object(id) {
            Object::String(s) => Some(s),
            _ => None,
        }
    }

    /// Whether the instance's class or one of its superclasses is called `name`.
    pub fn is_a(&self, id: ObjId, name: &str) -> bool {
        match self.object(id) {
            Object::Instance { data, .. } => data.iter().any(|d| *d.class == *name),
            _ => self.class_name(id) == Some(name),
        }
    }

    /// A field of an instance, looked up from the object's own class upwards, so a field of a
    /// subclass hides an equally named one of a superclass.
    pub fn field(&self, id: ObjId, name: &str) -> Option<&Value> {
        match self.object(id) {
            Object::Instance { data, .. } => data
                .iter()
                .rev()
                .flat_map(|d| d.fields.iter())
                .find(|(n, _)| **n == *name)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// A `boolean` field, `None` when it is missing or of another type.
    pub fn bool_field(&self, id: ObjId, name: &str) -> Option<bool> {
        self.field(id, name).and_then(Value::as_bool)
    }

    /// All fields called `name`, from the object's own class upwards. Classes of a hierarchy
    /// may each declare a field of the same name.
    pub fn fields<'a>(&'a self, id: ObjId, name: &'a str) -> impl Iterator<Item = &'a Value> {
        let data: &[ClassData] = match self.object(id) {
            Object::Instance { data, .. } => data,
            _ => &[],
        };
        data.iter()
            .rev()
            .flat_map(|d| d.fields.iter())
            .filter(move |(n, _)| **n == *name)
            .map(|(_, v)| v)
    }

    /// The annotation written by `class`'s own `writeObject`.
    pub fn annotation(&self, id: ObjId, class: &str) -> Option<&[Content]> {
        match self.object(id) {
            Object::Instance { data, .. } => {
                data.iter().find(|d| *d.class == *class).map(|d| d.annotation.as_slice())
            }
            _ => None,
        }
    }

    pub fn array(&self, id: ObjId) -> Option<&[Value]> {
        match self.object(id) {
            Object::Array { items, .. } => Some(items),
            _ => None,
        }
    }

    pub fn add(&mut self, object: Object) -> ObjId {
        self.objects.push(object);
        ObjId(self.objects.len() - 1)
    }

    pub fn add_string(&mut self, s: impl Into<String>) -> ObjId {
        self.add(Object::String(s.into()))
    }

    /// The first class description called `name`.
    pub fn class_named(&self, name: &str) -> Option<ClassId> {
        self.classes.iter().position(|c| *c.name == *name).map(ClassId)
    }

    /// A field of an instance for changing, looked up like [`Graph::field`].
    pub fn field_mut(&mut self, id: ObjId, name: &str) -> Option<&mut Value> {
        match &mut self.objects[id.0] {
            Object::Instance { data, .. } => data
                .iter_mut()
                .rev()
                .flat_map(|d| d.fields.iter_mut())
                .find(|(n, _)| **n == *name)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a field of an instance; `false` if it has no such field.
    pub fn set_field(&mut self, id: ObjId, name: &str, value: Value) -> bool {
        self.field_mut(id, name).map(|v| *v = value).is_some()
    }

    /// Sets the field of this name that the topmost class declares, where a subclass declares
    /// an unused one of the same name too; `false` if there is none.
    pub fn set_top_field(&mut self, id: ObjId, name: &str, value: Value) -> bool {
        let Object::Instance { data, .. } = &mut self.objects[id.0] else { return false };
        let field = data.iter_mut().flat_map(|d| d.fields.iter_mut()).find(|(n, _)| **n == *name);
        field.map(|(_, v)| *v = value).is_some()
    }

    /// The annotation written by `class`'s own `writeObject`, for changing.
    pub fn annotation_mut(&mut self, id: ObjId, class: &str) -> Option<&mut Vec<Content>> {
        match &mut self.objects[id.0] {
            Object::Instance { data, .. } => {
                data.iter_mut().find(|d| *d.class == *class).map(|d| &mut d.annotation)
            }
            _ => None,
        }
    }

    /// Copies the class description `class` of `from`, with its superclasses, unless this graph
    /// has one of the same name already.
    pub fn copy_class(&mut self, from: &Graph, class: ClassId) -> ClassId {
        let desc = from.class(class);
        if let Some(id) = self.class_named(&desc.name) {
            return id;
        }
        let super_class = desc.super_class.map(|c| self.copy_class(from, c));
        let mut copy = desc.clone();
        copy.super_class = super_class;
        let mut memo = HashMap::new();
        for field in &mut copy.fields {
            if let Some(type_name) = &field.type_name {
                field.type_name = Some(self.copy_value(from, type_name, &mut memo));
            }
        }
        copy.annotation = self.copy_contents(from, &desc.annotation, &mut memo);
        self.classes.push(copy);
        ClassId(self.classes.len() - 1)
    }

    /// Copies a value of `from` and every object it reaches into this graph. Objects met twice
    /// are copied once; `memo` maps the objects of `from` to their copies.
    pub fn copy_value(
        &mut self,
        from: &Graph,
        value: &Value,
        memo: &mut HashMap<ObjId, ObjId>,
    ) -> Value {
        let Value::Object(id) = *value else { return value.clone() };
        if let Some(&copy) = memo.get(&id) {
            return Value::Object(copy);
        }
        // Reserve the copy first, so cycles end at it.
        let copy = self.add(Object::String(String::new()));
        memo.insert(id, copy);
        let object = match from.object(id) {
            Object::String(s) => Object::String(s.clone()),
            Object::Instance { class, data } => {
                let class = self.copy_class(from, *class);
                let data = data
                    .iter()
                    .map(|d| ClassData {
                        class: d.class.clone(),
                        fields: d
                            .fields
                            .iter()
                            .map(|(n, v)| (n.clone(), self.copy_value(from, v, memo)))
                            .collect(),
                        annotation: self.copy_contents(from, &d.annotation, memo),
                    })
                    .collect();
                Object::Instance { class, data }
            }
            Object::Array { class, items } => Object::Array {
                class: self.copy_class(from, *class),
                items: items.iter().map(|v| self.copy_value(from, v, memo)).collect(),
            },
            Object::Enum { class, constant, name } => Object::Enum {
                class: self.copy_class(from, *class),
                constant: constant.clone(),
                name: self.copy_value(from, name, memo),
            },
            Object::Class(class) => Object::Class(self.copy_class(from, *class)),
        };
        self.objects[copy.0] = object;
        Value::Object(copy)
    }

    fn copy_contents(
        &mut self,
        from: &Graph,
        items: &[Content],
        memo: &mut HashMap<ObjId, ObjId>,
    ) -> Vec<Content> {
        items
            .iter()
            .map(|c| match c {
                Content::Value(v) => Content::Value(self.copy_value(from, v, memo)),
                other => other.clone(),
            })
            .collect()
    }

    /// The elements of an array or a standard Java collection. Maps yield key, value, key,
    /// value … Collections of unknown classes are empty.
    pub fn collection(&self, id: ObjId) -> Vec<&Value> {
        if let Some(items) = self.array(id) {
            return items.iter().collect();
        }
        let Object::Instance { data, .. } = self.object(id) else { return Vec::new() };
        if let (Some(Value::Object(array)), Some(count)) =
            (self.field(id, "elementData"), self.field(id, "elementCount"))
        {
            // java.util.Vector writes its backing array as a plain field.
            let count = count.as_i64().unwrap_or(0).max(0) as usize;
            return self.array(*array).map_or(Vec::new(), |a| a.iter().take(count).collect());
        }
        // ArrayList, LinkedList, HashMap, HashSet, Hashtable and their subclasses write a few
        // sizes as block data followed by the elements.
        const COLLECTIONS: &[&str] = &[
            "java.util.ArrayList",
            "java.util.LinkedList",
            "java.util.HashMap",
            "java.util.HashSet",
            "java.util.Hashtable",
            "java.util.TreeMap",
            "java.util.TreeSet",
        ];
        data.iter()
            .filter(|d| COLLECTIONS.contains(&&*d.class))
            .flat_map(|d| &d.annotation)
            .filter_map(|c| match c {
                Content::Value(v) => Some(v),
                Content::Block(_) | Content::Reset => None,
            })
            .collect()
    }
}

/// Reads the primitive data and objects of an annotation in the order they were written.
pub struct Annotation<'a> {
    items: &'a [Content],
    index: usize,
    block: &'a [u8],
}

impl<'a> Annotation<'a> {
    pub fn new(items: &'a [Content]) -> Self {
        Self { items, index: 0, block: &[] }
    }

    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut out = [0; N];
        let mut filled = 0;
        while filled < N {
            if self.block.is_empty() {
                match self.items.get(self.index)? {
                    Content::Block(bytes) => self.block = bytes,
                    Content::Value(_) | Content::Reset => return None,
                }
                self.index += 1;
            }
            let n = (N - filled).min(self.block.len());
            out[filled..filled + n].copy_from_slice(&self.block[..n]);
            self.block = &self.block[n..];
            filled += n;
        }
        Some(out)
    }

    pub fn int(&mut self) -> Option<i32> {
        self.bytes::<4>().map(i32::from_be_bytes)
    }

    /// The next object, skipping any primitive data still left in the current block.
    pub fn value(&mut self) -> Option<&'a Value> {
        self.block = &[];
        loop {
            let item = self.items.get(self.index)?;
            self.index += 1;
            if let Content::Value(v) = item {
                return Some(v);
            }
        }
    }
}

const TC_NULL: u8 = 0x70;
const TC_REFERENCE: u8 = 0x71;
const TC_CLASSDESC: u8 = 0x72;
const TC_OBJECT: u8 = 0x73;
const TC_STRING: u8 = 0x74;
const TC_ARRAY: u8 = 0x75;
const TC_CLASS: u8 = 0x76;
const TC_BLOCKDATA: u8 = 0x77;
const TC_ENDBLOCKDATA: u8 = 0x78;
const TC_RESET: u8 = 0x79;
const TC_BLOCKDATALONG: u8 = 0x7A;
const TC_LONGSTRING: u8 = 0x7C;
const TC_PROXYCLASSDESC: u8 = 0x7D;
const TC_ENUM: u8 = 0x7E;
const BASE_HANDLE: u32 = 0x7E_0000;
const MAGIC: u16 = 0xACED;
const VERSION: u16 = 5;

/// Deeper object graphs are rejected instead of overflowing the stack.
const MAX_DEPTH: usize = 20_000;

/// Longest superclass chain accepted. Astah's model classes have about a dozen ancestors; the
/// limit keeps every instance from carrying thousands of class records.
const MAX_CLASS_CHAIN: usize = 64;

/// What a wire handle stands for.
#[derive(Clone, Copy)]
enum Handle {
    Class(ClassId),
    Object(ObjId),
}

/// What the graph may take in memory, in bytes per byte of stream, plus a fixed allowance.
/// Every item the reader decodes costs at least one byte of stream, but its record takes 16 to
/// 72 bytes, and a six-byte reference to a class brings a record for each class of its
/// hierarchy. Astah's own projects and templates take 9.5 to 13 bytes per byte. The budget
/// refuses a crafted stream (a long run of `null`s, instances of deep hierarchies) before it
/// exhausts memory, which would abort the process instead of failing.
const BUDGET_PER_BYTE: usize = 24;
const BUDGET_BASE: usize = 64 << 20;
/// The most the graph may take whatever the stream's size: the largest store read (256 MB)
/// would otherwise allow 6 GB, which a few hundred KB of zipped `null`s reach. Enough for a
/// stream of about 40 MB at Astah's 13 bytes per byte.
const BUDGET_MAX: usize = 512 << 20;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    handles: Vec<Handle>,
    graph: Graph,
    depth: usize,
    /// Bytes the graph may still take; see [`BUDGET_PER_BYTE`].
    budget: usize,
}

/// Parses a complete stream. `extra_header` is the number of bytes a custom
/// `writeStreamHeader` wrote after the standard magic and version.
pub fn parse(data: &[u8], extra_header: usize) -> Result<Graph> {
    let budget = data.len().saturating_mul(BUDGET_PER_BYTE).saturating_add(BUDGET_BASE);
    parse_with_budget(data, extra_header, budget.min(BUDGET_MAX))
}

/// [`parse`] with `budget` bytes for the graph.
fn parse_with_budget(data: &[u8], extra_header: usize, budget: usize) -> Result<Graph> {
    let mut r =
        Reader { data, pos: 0, handles: Vec::new(), graph: Graph::default(), depth: 0, budget };
    if r.u16()? != MAGIC || r.u16()? != VERSION {
        return Err(StreamError::BadMagic);
    }
    r.take(extra_header)?;
    while r.pos < data.len() {
        let content = r.content()?;
        r.graph.roots.push(content);
    }
    Ok(r.graph)
}

impl<'a> Reader<'a> {
    /// Takes `bytes` from the budget, or refuses the stream.
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.budget = self.budget.checked_sub(bytes).ok_or(StreamError::TooLarge)?;
        Ok(())
    }

    /// Charges `n` records of type `T`.
    fn charge_for<T>(&mut self, n: usize) -> Result<()> {
        self.charge(n.saturating_mul(std::mem::size_of::<T>()))
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or(StreamError::Eof(self.pos))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("took N bytes"))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn peek(&self) -> Result<u8> {
        self.data.get(self.pos).copied().ok_or(StreamError::Eof(self.pos))
    }

    fn u16(&mut self) -> Result<u16> {
        self.array().map(u16::from_be_bytes)
    }

    fn i32(&mut self) -> Result<i32> {
        self.array().map(i32::from_be_bytes)
    }

    fn i64(&mut self) -> Result<i64> {
        self.array().map(i64::from_be_bytes)
    }

    fn utf(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        let s = decode_modified_utf8(self.take(len)?);
        self.charge(s.len())?;
        Ok(s)
    }

    fn long_utf(&mut self) -> Result<String> {
        let len = self.i64()?;
        let len = usize::try_from(len).map_err(|_| StreamError::Eof(self.pos))?;
        let s = decode_modified_utf8(self.take(len)?);
        self.charge(s.len())?;
        Ok(s)
    }

    /// A wire handle, and what it stands for.
    fn handle(&mut self) -> Result<(u32, Handle)> {
        let raw = self.i32()? as u32;
        let index = raw.checked_sub(BASE_HANDLE).ok_or(StreamError::BadHandle(raw))?;
        let handle = self.handles.get(index as usize).copied();
        Ok((raw, handle.ok_or(StreamError::BadHandle(raw))?))
    }

    fn new_class(&mut self, desc: ClassDesc) -> Result<ClassId> {
        self.charge_for::<ClassDesc>(1)?;
        self.charge_for::<Handle>(1)?;
        let id = ClassId(self.graph.classes.len());
        self.graph.classes.push(desc);
        self.handles.push(Handle::Class(id));
        Ok(id)
    }

    fn new_object(&mut self, placeholder: Object) -> Result<ObjId> {
        self.charge_for::<Object>(1)?;
        self.charge_for::<Handle>(1)?;
        let id = ObjId(self.graph.objects.len());
        self.graph.objects.push(placeholder);
        self.handles.push(Handle::Object(id));
        Ok(id)
    }

    /// One item of the stream or of an annotation.
    fn content(&mut self) -> Result<Content> {
        self.charge_for::<Content>(1)?;
        match self.peek()? {
            TC_BLOCKDATA => {
                self.pos += 1;
                let len = self.u8()? as usize;
                let bytes = self.take(len)?;
                self.charge(len)?;
                Ok(Content::Block(bytes.to_vec()))
            }
            TC_BLOCKDATALONG => {
                self.pos += 1;
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| StreamError::Eof(self.pos))?;
                let bytes = self.take(len)?;
                self.charge(len)?;
                Ok(Content::Block(bytes.to_vec()))
            }
            TC_RESET => {
                self.pos += 1;
                self.handles.clear();
                Ok(Content::Reset)
            }
            _ => self.value().map(Content::Value),
        }
    }

    /// Contents up to and including the closing `TC_ENDBLOCKDATA`.
    fn annotation(&mut self) -> Result<Vec<Content>> {
        let mut items = Vec::new();
        while self.peek()? != TC_ENDBLOCKDATA {
            items.push(self.content()?);
        }
        self.pos += 1;
        Ok(items)
    }

    fn value(&mut self) -> Result<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(StreamError::TooDeep);
        }
        let result = self.value_inner();
        self.depth -= 1;
        result
    }

    fn value_inner(&mut self) -> Result<Value> {
        let at = self.pos;
        let code = self.u8()?;
        let id = match code {
            TC_NULL => return Ok(Value::Null),
            TC_REFERENCE => match self.handle()? {
                (_, Handle::Object(id)) => id,
                (raw, Handle::Class(_)) => return Err(StreamError::WrongHandle(raw, "object")),
            },
            TC_STRING => {
                let s = self.utf()?;
                self.new_object(Object::String(s))?
            }
            TC_LONGSTRING => {
                let s = self.long_utf()?;
                self.new_object(Object::String(s))?
            }
            TC_OBJECT => self.instance()?,
            TC_ARRAY => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                let id = self.new_object(Object::Array { class, items: Vec::new() })?;
                let element = array_element(&self.graph.class(class).name)?;
                let len = usize::try_from(self.i32()?).map_err(|_| StreamError::Eof(self.pos))?;
                self.charge_for::<Value>(len)?;
                let mut items = Vec::with_capacity(len.min(1 << 16));
                for _ in 0..len {
                    items.push(self.typed(element)?);
                }
                self.graph.objects[id.0] = Object::Array { class, items };
                id
            }
            TC_ENUM => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                let placeholder =
                    Object::Enum { class, constant: String::new(), name: Value::Null };
                let id = self.new_object(placeholder)?;
                let name = self.value()?;
                let constant = match name {
                    Value::Object(s) => self.graph.string(s).unwrap_or_default().to_owned(),
                    _ => String::new(),
                };
                self.graph.objects[id.0] = Object::Enum { class, constant, name };
                id
            }
            TC_CLASS => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                self.new_object(Object::Class(class))?
            }
            _ => return Err(StreamError::BadTypeCode { code, at }),
        };
        Ok(Value::Object(id))
    }

    fn typed(&mut self, ty: FieldType) -> Result<Value> {
        Ok(match ty {
            FieldType::Byte => Value::Byte(self.u8()? as i8),
            FieldType::Char => Value::Char(self.u16()?),
            FieldType::Double => Value::Double(f64::from_bits(self.i64()? as u64)),
            FieldType::Float => Value::Float(f32::from_bits(self.i32()? as u32)),
            FieldType::Int => Value::Int(self.i32()?),
            FieldType::Long => Value::Long(self.i64()?),
            FieldType::Short => Value::Short(self.u16()? as i16),
            FieldType::Bool => Value::Bool(self.u8()? != 0),
            FieldType::Object => self.value()?,
        })
    }

    fn instance(&mut self) -> Result<ObjId> {
        let at = self.pos;
        let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code: TC_NULL, at })?;
        let id = self.new_object(Object::Instance { class, data: Vec::new() })?;
        let mut hierarchy = Vec::new();
        let mut current = Some(class);
        while let Some(c) = current {
            hierarchy.push(c);
            current = self.graph.class(c).super_class;
        }
        self.charge_for::<ClassData>(hierarchy.len())?;
        let mut data = Vec::with_capacity(hierarchy.len());
        for &c in hierarchy.iter().rev() {
            // Names are shared with the description, not copied into every instance.
            let desc = self.graph.class(c);
            let (flags, count) = (desc.flags, desc.fields.len());
            let mut class_data = ClassData { class: desc.name.clone(), ..Default::default() };
            if flags & SC_EXTERNALIZABLE != 0 {
                if flags & SC_BLOCK_DATA == 0 {
                    return Err(StreamError::Externalizable(class_data.class.to_string()));
                }
                class_data.annotation = self.annotation()?;
            } else if flags & SC_SERIALIZABLE != 0 {
                self.charge_for::<(Arc<str>, Value)>(count)?;
                class_data.fields.reserve(count);
                for i in 0..self.graph.class(c).fields.len() {
                    let field = &self.graph.class(c).fields[i];
                    let (name, ty) = (field.name.clone(), field.ty());
                    let value = self.typed(ty)?;
                    class_data.fields.push((name, value));
                }
                if flags & SC_WRITE_METHOD != 0 {
                    class_data.annotation = self.annotation()?;
                }
            }
            data.push(class_data);
        }
        self.graph.objects[id.0] = Object::Instance { class, data };
        Ok(id)
    }

    /// A class description, a reference to one, or `null`. Nested descriptions (superclasses)
    /// count toward [`MAX_DEPTH`] like nested values.
    fn class_desc(&mut self) -> Result<Option<ClassId>> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(StreamError::TooDeep);
        }
        let result = self.class_desc_inner();
        self.depth -= 1;
        result
    }

    /// Sets the superclass of `id`, rejecting a chain that leads back to `id` or is longer than
    /// [`MAX_CLASS_CHAIN`]. Every chain is checked when it is built, so walking it later ends.
    fn set_super_class(&mut self, id: ClassId, super_class: Option<ClassId>) -> Result<()> {
        let mut current = super_class;
        let mut length = 1;
        while let Some(c) = current {
            let name = || self.graph.class(id).name.clone();
            if c == id {
                return Err(StreamError::ClassCycle(name().to_string()));
            }
            length += 1;
            if length > MAX_CLASS_CHAIN {
                return Err(StreamError::ClassChainTooLong(name().to_string()));
            }
            current = self.graph.class(c).super_class;
        }
        self.graph.classes[id.0].super_class = super_class;
        Ok(())
    }

    fn class_desc_inner(&mut self) -> Result<Option<ClassId>> {
        let at = self.pos;
        match self.u8()? {
            TC_NULL => Ok(None),
            TC_REFERENCE => match self.handle()? {
                (_, Handle::Class(id)) => Ok(Some(id)),
                (raw, Handle::Object(_)) => Err(StreamError::WrongHandle(raw, "class description")),
            },
            TC_CLASSDESC => {
                let name = self.utf()?;
                let serial_version_uid = self.i64()?;
                // The handle is assigned before the rest of the description is read.
                let id = self.new_class(ClassDesc::new(name, serial_version_uid, 0))?;
                let flags = self.u8()?;
                let count = self.u16()?;
                self.charge_for::<FieldDesc>(count as usize)?;
                let mut fields = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let code = self.u8()?;
                    let ty = FieldType::from_code(code)?;
                    let name = self.utf()?;
                    let type_name =
                        if ty == FieldType::Object { Some(self.value()?) } else { None };
                    fields.push(FieldDesc { name: name.into(), code, type_name });
                }
                let annotation = self.annotation()?;
                let super_class = self.class_desc()?;
                let class = &mut self.graph.classes[id.0];
                class.flags = flags;
                class.fields = fields;
                class.annotation = annotation;
                self.set_super_class(id, super_class)?;
                Ok(Some(id))
            }
            TC_PROXYCLASSDESC => {
                let id = self.new_class(ClassDesc::new("$Proxy", 0, SC_SERIALIZABLE))?;
                let count = self.i32()?;
                let mut interfaces = Vec::new();
                for _ in 0..count {
                    self.charge_for::<String>(1)?;
                    interfaces.push(self.utf()?);
                }
                let annotation = self.annotation()?;
                let super_class = self.class_desc()?;
                let class = &mut self.graph.classes[id.0];
                class.interfaces = Some(interfaces);
                class.annotation = annotation;
                self.set_super_class(id, super_class)?;
                Ok(Some(id))
            }
            code => Err(StreamError::BadTypeCode { code, at }),
        }
    }
}

/// Java's "modified UTF-8": NUL as two bytes and supplementary characters as surrogate pairs.
fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i] as u16;
        let cont = |k: usize| bytes.get(i + k).map_or(0, |&c| (c & 0x3F) as u16);
        if b < 0x80 {
            units.push(b);
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            units.push(((b & 0x1F) << 6) | cont(1));
            i += 2;
        } else {
            units.push(((b & 0x0F) << 12) | (cont(1) << 6) | cont(2));
            i += 3;
        }
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf8() {
        assert_eq!(decode_modified_utf8(b"abc"), "abc");
        assert_eq!(decode_modified_utf8(&[0xC0, 0x80]), "\0");
        assert_eq!(decode_modified_utf8("Größe".as_bytes()), "Größe");
        // U+1F600 as a surrogate pair, each half encoded on its own.
        let smiley = [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        assert_eq!(decode_modified_utf8(&smiley), "😀");
    }

    /// `new java.util.ArrayList<>(List.of("a"))` as written by `ObjectOutputStream`.
    fn array_list_bytes() -> Vec<u8> {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT, TC_CLASSDESC];
        let name = b"java.util.ArrayList";
        bytes.extend((name.len() as u16).to_be_bytes());
        bytes.extend(name);
        bytes.extend(0x7881_D21D_99C7_619D_u64.to_be_bytes());
        bytes.push(SC_SERIALIZABLE | SC_WRITE_METHOD);
        bytes.extend(1u16.to_be_bytes());
        bytes.push(b'I');
        bytes.extend(4u16.to_be_bytes());
        bytes.extend(b"size");
        bytes.extend([TC_ENDBLOCKDATA, TC_NULL]);
        bytes.extend(1i32.to_be_bytes());
        bytes.extend([TC_BLOCKDATA, 4]);
        bytes.extend(1i32.to_be_bytes());
        bytes.extend([TC_STRING, 0, 1, b'a', TC_ENDBLOCKDATA]);
        bytes
    }

    #[test]
    fn array_list() {
        let bytes = array_list_bytes();
        let graph = parse(&bytes, 0).unwrap();
        let Some(Content::Value(Value::Object(list))) = graph.roots.first() else {
            panic!("no root object");
        };
        assert_eq!(graph.class_name(*list), Some("java.util.ArrayList"));
        assert_eq!(graph.field(*list, "size"), Some(&Value::Int(1)));
        let mut a = Annotation::new(graph.annotation(*list, "java.util.ArrayList").unwrap());
        assert_eq!(a.int(), Some(1));
        let item = a.value().and_then(Value::as_object).unwrap();
        assert_eq!(graph.string(item), Some("a"));
        assert_eq!(write(&graph, &[]).unwrap(), bytes);
    }

    /// The start of a serializable class description named `name` without fields.
    fn class_desc_head(bytes: &mut Vec<u8>, name: &[u8]) {
        bytes.push(TC_CLASSDESC);
        bytes.extend((name.len() as u16).to_be_bytes());
        bytes.extend(name);
        bytes.extend(1u64.to_be_bytes());
        bytes.push(SC_SERIALIZABLE);
        bytes.extend(0u16.to_be_bytes());
        bytes.push(TC_ENDBLOCKDATA);
    }

    /// Every byte of a stream replaced in turn by type codes, extremes and its neighbors, and
    /// every prefix of it: the reader returns a graph or an error, never panics, and what it
    /// returns can be written again. A deterministic stand-in for fuzzing.
    #[test]
    fn damaged_streams_are_read_or_refused() {
        let bytes = array_list_bytes();
        for len in 0..bytes.len() {
            let _ = parse(&bytes[..len], 0);
        }
        let values = [0x00, 0x01, 0x7F, 0x80, 0xFF, TC_NULL, TC_REFERENCE, TC_CLASSDESC, TC_OBJECT];
        for at in 0..bytes.len() {
            let mut variants: Vec<u8> = values.to_vec();
            variants.extend([bytes[at].wrapping_add(1), bytes[at].wrapping_sub(1)]);
            for value in variants {
                let mut damaged = bytes.clone();
                damaged[at] = value;
                if let Ok(graph) = parse(&damaged, 0) {
                    let _ = write(&graph, &[]);
                }
            }
        }
    }

    /// The class of the stream's first object.
    fn root_class(graph: &Graph) -> &ClassDesc {
        let Some(Content::Value(Value::Object(id))) = graph.roots.first() else {
            panic!("no root object");
        };
        let Object::Instance { class, .. } = &graph.objects[id.0] else { panic!("no instance") };
        graph.class(*class)
    }

    #[test]
    fn proxy_classes_keep_their_interfaces() {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT, TC_PROXYCLASSDESC];
        bytes.extend(2i32.to_be_bytes());
        for name in [&b"a.I"[..], b"a.J"] {
            bytes.extend((name.len() as u16).to_be_bytes());
            bytes.extend(name);
        }
        bytes.extend([TC_ENDBLOCKDATA, TC_NULL]);
        let graph = parse(&bytes, 0).unwrap();
        let class = root_class(&graph);
        assert_eq!(&*class.name, "$Proxy");
        assert_eq!(class.interfaces.as_deref(), Some(&["a.I".to_owned(), "a.J".to_owned()][..]));
        assert_eq!(write(&graph, &[]).unwrap(), bytes);
    }

    /// An object of an externalizable class `E` that writes its data in blocks, as protocol 2
    /// does, followed by `block`.
    fn externalizable(flags: u8, block: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT, TC_CLASSDESC, 0, 1, b'E'];
        bytes.extend(1u64.to_be_bytes());
        bytes.push(flags);
        bytes.extend(0u16.to_be_bytes());
        bytes.extend([TC_ENDBLOCKDATA, TC_NULL]);
        bytes.extend(block);
        bytes
    }

    #[test]
    fn externalizable_objects_keep_their_block_data() {
        let flags = SC_EXTERNALIZABLE | SC_BLOCK_DATA;
        let bytes = externalizable(flags, &[TC_BLOCKDATA, 2, 7, 9, TC_ENDBLOCKDATA]);
        let graph = parse(&bytes, 0).unwrap();
        let Some(Content::Value(Value::Object(id))) = graph.roots.first() else { panic!() };
        let annotation = graph.annotation(*id, "E").unwrap();
        assert!(matches!(annotation, [Content::Block(b)] if b == &[7, 9]), "{annotation:?}");
        assert_eq!(write(&graph, &[]).unwrap(), bytes);
        // Protocol 1 writes the data raw, of a length only the class knows: not readable.
        let raw = externalizable(SC_EXTERNALIZABLE, &[7, 9]);
        assert!(matches!(parse(&raw, 0), Err(StreamError::Externalizable(name)) if name == "E"));
    }

    #[test]
    fn a_class_that_is_its_own_superclass_is_rejected() {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT];
        class_desc_head(&mut bytes, b"A");
        bytes.push(TC_REFERENCE);
        bytes.extend(BASE_HANDLE.to_be_bytes());
        assert!(matches!(parse(&bytes, 0), Err(StreamError::ClassCycle(name)) if name == "A"));
    }

    #[test]
    fn long_superclass_chains_are_rejected() {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT];
        for _ in 0..MAX_CLASS_CHAIN + 1 {
            class_desc_head(&mut bytes, b"C");
        }
        bytes.push(TC_NULL);
        assert!(matches!(parse(&bytes, 0), Err(StreamError::ClassChainTooLong(_))));
    }

    /// Instances of a class with a long hierarchy, each six bytes of stream after the first
    /// and a record per class in memory: 600 kB that would take 400 MB are refused.
    #[test]
    fn streams_that_decode_to_too_much_are_refused() {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT];
        for _ in 0..MAX_CLASS_CHAIN - 1 {
            class_desc_head(&mut bytes, b"C");
        }
        bytes.push(TC_NULL);
        for _ in 0..100_000 {
            bytes.extend([TC_OBJECT, TC_REFERENCE]);
            bytes.extend(BASE_HANDLE.to_be_bytes());
        }
        assert!(matches!(parse(&bytes, 0), Err(StreamError::TooLarge)));
        // A tenth of them, and the first, is within the budget.
        let few = bytes.len() - 90_000 * 6;
        assert_eq!(parse(&bytes[..few], 0).unwrap().roots.len(), 10_001);
        // A run of `null`s, each a byte of stream and a record of 32 bytes.
        let mut nulls = MAGIC.to_vec();
        nulls.resize(MAGIC.len() + 100_000, TC_NULL);
        assert!(matches!(parse_with_budget(&nulls, 0, 1 << 20), Err(StreamError::TooLarge)));
        assert_eq!(parse_with_budget(&nulls, 0, 4 << 20).unwrap().roots.len(), 100_000);
    }

    #[test]
    fn deeply_nested_class_descriptions_are_rejected() {
        // Superclasses are read before their chain is checked, so the nesting itself is limited.
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_OBJECT];
        for _ in 0..MAX_DEPTH {
            class_desc_head(&mut bytes, b"D");
        }
        let result = std::thread::Builder::new()
            .stack_size(512 << 20)
            .spawn(move || parse(&bytes, 0))
            .unwrap()
            .join()
            .unwrap();
        assert!(matches!(result, Err(StreamError::TooDeep)));
    }

    #[test]
    fn a_wrong_handle_names_the_handle() {
        let mut bytes = vec![0xAC, 0xED, 0x00, 0x05, TC_STRING, 0, 1, b'a', TC_OBJECT];
        bytes.push(TC_REFERENCE);
        bytes.extend(BASE_HANDLE.to_be_bytes());
        let error = parse(&bytes, 0).unwrap_err();
        assert!(matches!(error, StreamError::WrongHandle(BASE_HANDLE, _)), "{error}");
    }

    const MAGIC: [u8; 4] = [0xAC, 0xED, 0x00, 0x05];

    fn root(graph: &Graph, i: usize) -> &Object {
        let Some(Content::Value(Value::Object(id))) = graph.roots.get(i) else {
            panic!("root {i} is no object: {:?}", graph.roots.get(i))
        };
        &graph.objects[id.0]
    }

    #[test]
    fn long_strings_reset_classes_enums_and_primitive_arrays() {
        let mut bytes = MAGIC.to_vec();
        // A long string, then a reset: the next handle starts again at the base.
        bytes.push(TC_LONGSTRING);
        bytes.extend(3u64.to_be_bytes());
        bytes.extend(b"xyz");
        bytes.push(TC_RESET);
        // `int[].class`, then an `int[] { 7, -1 }` that reuses its description by handle.
        bytes.push(TC_CLASS);
        class_desc_head(&mut bytes, b"[I");
        bytes.push(TC_NULL);
        bytes.push(TC_ARRAY);
        bytes.push(TC_REFERENCE);
        bytes.extend(BASE_HANDLE.to_be_bytes());
        bytes.extend(2i32.to_be_bytes());
        bytes.extend(7i32.to_be_bytes());
        bytes.extend((-1i32).to_be_bytes());
        // An enum constant `Kind.B`.
        bytes.extend([TC_ENUM, TC_CLASSDESC]);
        bytes.extend(4u16.to_be_bytes());
        bytes.extend(b"Kind");
        bytes.extend(0u64.to_be_bytes());
        bytes.push(SC_SERIALIZABLE | SC_ENUM);
        bytes.extend(0u16.to_be_bytes());
        bytes.extend([TC_ENDBLOCKDATA, TC_NULL]);
        bytes.extend([TC_STRING, 0, 1, b'B']);

        let graph = parse(&bytes, 0).unwrap();
        assert!(matches!(root(&graph, 0), Object::String(s) if s == "xyz"));
        assert!(matches!(graph.roots[1], Content::Reset));
        let Object::Class(class) = root(&graph, 2) else { panic!() };
        assert_eq!(&*graph.classes[class.0].name, "[I");
        let Object::Array { items, .. } = root(&graph, 3) else { panic!() };
        assert_eq!(items, &[Value::Int(7), Value::Int(-1)]);
        let Object::Enum { constant, .. } = root(&graph, 4) else { panic!() };
        assert_eq!(constant, "B");
        // Written back as Java writes it: a string this short as `TC_STRING`.
        let mut short = MAGIC.to_vec();
        short.extend([TC_STRING, 0, 3]);
        short.extend(&bytes[MAGIC.len() + 9..]);
        assert_eq!(write(&graph, &[]).unwrap(), short);
    }

    #[test]
    fn broken_streams_say_what_is_wrong() {
        assert!(matches!(parse(b"\xAC\xED\x00\x06", 0), Err(StreamError::BadMagic)));
        let mut cut = MAGIC.to_vec();
        cut.extend([TC_STRING, 0, 5, b'a']);
        assert!(matches!(parse(&cut, 0), Err(StreamError::Eof(_))));
        let mut unknown = MAGIC.to_vec();
        unknown.push(TC_REFERENCE);
        unknown.extend((BASE_HANDLE + 9).to_be_bytes());
        assert!(
            matches!(parse(&unknown, 0), Err(StreamError::BadHandle(h)) if h == BASE_HANDLE + 9)
        );
        let mut code = MAGIC.to_vec();
        code.push(0x42);
        assert!(matches!(parse(&code, 0), Err(StreamError::BadTypeCode { code: 0x42, at: 4 })));
    }
}
