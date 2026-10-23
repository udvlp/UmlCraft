//! Reader for the Java Object Serialization Stream Protocol.
//!
//! Astah stores its model as one serialized object graph. This module knows nothing about
//! Astah; it turns the stream into a [`Graph`] of generic objects that the importer then reads
//! by class and field name.
//!
//! Data written by custom `writeObject` methods ends up in [`ClassData::annotation`] as raw
//! block data and embedded objects; [`Annotation`] reads it back in order.

use std::collections::HashMap;

use thiserror::Error;

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
}

type Result<T> = std::result::Result<T, StreamError>;

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
enum FieldType {
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
    fn from_code(code: u8) -> Result<Self> {
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
    pub name: String,
    flags: u8,
    fields: Vec<(String, FieldType)>,
    pub super_class: Option<ClassId>,
}

const SC_WRITE_METHOD: u8 = 0x01;
const SC_SERIALIZABLE: u8 = 0x02;
const SC_EXTERNALIZABLE: u8 = 0x04;
const SC_BLOCK_DATA: u8 = 0x08;

/// The serialized state one class of an object's hierarchy contributed.
#[derive(Clone, Debug, Default)]
pub struct ClassData {
    pub class: String,
    pub fields: Vec<(String, Value)>,
    /// What a custom `writeObject` wrote after the default fields.
    pub annotation: Vec<Content>,
}

/// One item of an annotation: raw block data or an object (possibly `null`).
#[derive(Clone, Debug)]
pub enum Content {
    Block(Vec<u8>),
    Value(Value),
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
            Object::Instance { data, .. } => data.iter().any(|d| d.class == name),
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
                .find(|(n, _)| n == name)
                .map(|(_, v)| v),
            _ => None,
        }
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
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// The annotation written by `class`'s own `writeObject`.
    pub fn annotation(&self, id: ObjId, class: &str) -> Option<&[Content]> {
        match self.object(id) {
            Object::Instance { data, .. } => {
                data.iter().find(|d| d.class == class).map(|d| d.annotation.as_slice())
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
            .filter(|d| COLLECTIONS.contains(&d.class.as_str()))
            .flat_map(|d| &d.annotation)
            .filter_map(|c| match c {
                Content::Value(v) => Some(v),
                Content::Block(_) => None,
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
                    Content::Value(_) => return None,
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

/// Deeper object graphs are rejected instead of overflowing the stack.
const MAX_DEPTH: usize = 20_000;

/// What a wire handle stands for.
#[derive(Clone, Copy)]
enum Handle {
    Class(ClassId),
    Object(ObjId),
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    handles: Vec<Handle>,
    graph: Graph,
    depth: usize,
    /// Class descriptions by name, so a stream reset does not duplicate them.
    class_by_name: HashMap<String, ClassId>,
}

/// Parses a complete stream. `extra_header` is the number of bytes a custom
/// `writeStreamHeader` wrote after the standard magic and version.
pub fn parse(data: &[u8], extra_header: usize) -> Result<Graph> {
    let mut r = Reader {
        data,
        pos: 0,
        handles: Vec::new(),
        graph: Graph::default(),
        depth: 0,
        class_by_name: HashMap::new(),
    };
    if r.u16()? != 0xACED || r.u16()? != 5 {
        return Err(StreamError::BadMagic);
    }
    r.take(extra_header)?;
    while r.pos < data.len() {
        let content = r.content()?;
        if let Some(content) = content {
            r.graph.roots.push(content);
        }
    }
    Ok(r.graph)
}

impl<'a> Reader<'a> {
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
        Ok(decode_modified_utf8(self.take(len)?))
    }

    fn long_utf(&mut self) -> Result<String> {
        let len = self.i64()?;
        let len = usize::try_from(len).map_err(|_| StreamError::Eof(self.pos))?;
        Ok(decode_modified_utf8(self.take(len)?))
    }

    fn handle(&mut self) -> Result<Handle> {
        let raw = self.i32()? as u32;
        let index = raw.checked_sub(BASE_HANDLE).ok_or(StreamError::BadHandle(raw))?;
        self.handles.get(index as usize).copied().ok_or(StreamError::BadHandle(raw))
    }

    fn new_object(&mut self, placeholder: Object) -> ObjId {
        let id = ObjId(self.graph.objects.len());
        self.graph.objects.push(placeholder);
        self.handles.push(Handle::Object(id));
        id
    }

    /// One item of the stream or of an annotation; `None` for a reset.
    fn content(&mut self) -> Result<Option<Content>> {
        match self.peek()? {
            TC_BLOCKDATA => {
                self.pos += 1;
                let len = self.u8()? as usize;
                Ok(Some(Content::Block(self.take(len)?.to_vec())))
            }
            TC_BLOCKDATALONG => {
                self.pos += 1;
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| StreamError::Eof(self.pos))?;
                Ok(Some(Content::Block(self.take(len)?.to_vec())))
            }
            TC_RESET => {
                self.pos += 1;
                self.handles.clear();
                Ok(None)
            }
            _ => self.value().map(|v| Some(Content::Value(v))),
        }
    }

    /// Contents up to and including the closing `TC_ENDBLOCKDATA`.
    fn annotation(&mut self) -> Result<Vec<Content>> {
        let mut items = Vec::new();
        while self.peek()? != TC_ENDBLOCKDATA {
            items.extend(self.content()?);
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
                Handle::Object(id) => id,
                Handle::Class(_) => return Err(StreamError::WrongHandle(0, "object")),
            },
            TC_STRING => {
                let s = self.utf()?;
                self.new_object(Object::String(s))
            }
            TC_LONGSTRING => {
                let s = self.long_utf()?;
                self.new_object(Object::String(s))
            }
            TC_OBJECT => self.instance()?,
            TC_ARRAY => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                let id = self.new_object(Object::Array { class, items: Vec::new() });
                let element = self.graph.class(class).name.as_bytes().get(1).copied();
                let element = FieldType::from_code(element.unwrap_or(b'L'))?;
                let len = usize::try_from(self.i32()?).map_err(|_| StreamError::Eof(self.pos))?;
                let mut items = Vec::with_capacity(len.min(1 << 16));
                for _ in 0..len {
                    items.push(self.typed(element)?);
                }
                self.graph.objects[id.0] = Object::Array { class, items };
                id
            }
            TC_ENUM => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                let id = self.new_object(Object::Enum { class, constant: String::new() });
                let constant = match self.value()? {
                    Value::Object(s) => self.graph.string(s).unwrap_or_default().to_owned(),
                    _ => String::new(),
                };
                self.graph.objects[id.0] = Object::Enum { class, constant };
                id
            }
            TC_CLASS => {
                let class = self.class_desc()?.ok_or(StreamError::BadTypeCode { code, at })?;
                self.new_object(Object::Class(class))
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
        let id = self.new_object(Object::Instance { class, data: Vec::new() });
        let mut hierarchy = Vec::new();
        let mut current = Some(class);
        while let Some(c) = current {
            hierarchy.push(c);
            current = self.graph.class(c).super_class;
        }
        let mut data = Vec::with_capacity(hierarchy.len());
        for &c in hierarchy.iter().rev() {
            let desc = self.graph.class(c).clone();
            let mut class_data = ClassData { class: desc.name.clone(), ..Default::default() };
            if desc.flags & SC_EXTERNALIZABLE != 0 {
                if desc.flags & SC_BLOCK_DATA == 0 {
                    return Err(StreamError::Externalizable(desc.name));
                }
                class_data.annotation = self.annotation()?;
            } else if desc.flags & SC_SERIALIZABLE != 0 {
                for (name, ty) in &desc.fields {
                    let value = self.typed(*ty)?;
                    class_data.fields.push((name.clone(), value));
                }
                if desc.flags & SC_WRITE_METHOD != 0 {
                    class_data.annotation = self.annotation()?;
                }
            }
            data.push(class_data);
        }
        self.graph.objects[id.0] = Object::Instance { class, data };
        Ok(id)
    }

    /// A class description, a reference to one, or `null`.
    fn class_desc(&mut self) -> Result<Option<ClassId>> {
        let at = self.pos;
        match self.u8()? {
            TC_NULL => Ok(None),
            TC_REFERENCE => match self.handle()? {
                Handle::Class(id) => Ok(Some(id)),
                Handle::Object(_) => Err(StreamError::WrongHandle(0, "class description")),
            },
            TC_CLASSDESC => {
                let name = self.utf()?;
                let _serial_version_uid = self.i64()?;
                // The handle is assigned before the rest of the description is read.
                let id = ClassId(self.graph.classes.len());
                self.graph.classes.push(ClassDesc {
                    name: name.clone(),
                    flags: 0,
                    fields: Vec::new(),
                    super_class: None,
                });
                self.handles.push(Handle::Class(id));
                let flags = self.u8()?;
                let count = self.u16()?;
                let mut fields = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let ty = FieldType::from_code(self.u8()?)?;
                    let field_name = self.utf()?;
                    if ty == FieldType::Object {
                        // The field's class name, as a string object.
                        self.value()?;
                    }
                    fields.push((field_name, ty));
                }
                self.annotation()?;
                let super_class = self.class_desc()?;
                self.graph.classes[id.0] =
                    ClassDesc { name: name.clone(), flags, fields, super_class };
                self.class_by_name.entry(name).or_insert(id);
                Ok(Some(id))
            }
            TC_PROXYCLASSDESC => {
                let id = ClassId(self.graph.classes.len());
                self.graph.classes.push(ClassDesc {
                    name: "$Proxy".into(),
                    flags: SC_SERIALIZABLE,
                    fields: Vec::new(),
                    super_class: None,
                });
                self.handles.push(Handle::Class(id));
                let count = self.i32()?;
                for _ in 0..count {
                    self.utf()?;
                }
                self.annotation()?;
                let super_class = self.class_desc()?;
                self.graph.classes[id.0].super_class = super_class;
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
    #[test]
    fn array_list() {
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
    }
}
