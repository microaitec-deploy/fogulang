//! `fogu:protobuf` module — Protocol Buffers without code generation: load
//! `.proto` files (compiled in pure Rust by protox, no `protoc`) or a
//! descriptor set, then encode/decode messages from Fogu maps.
//!
//! ```fogu
//! import protobuf from "fogu:protobuf"
//!
//! const schema = protobuf.parse("syntax = \"proto3\"; package demo; message User { string name = 1; int32 age = 2; repeated string tags = 3; }")?
//! const bytes = schema.encode("demo.User", { name: "Ana", age: 30, tags: ["admin"] })?
//! print(schema.decode("demo.User", bytes)?)        // { age: 30, name: Ana, tags: [admin] }
//! const s2 = protobuf.load("api/user.proto", { includes: ["api"] })?
//! print(s2.messages, s2.services)
//! ```
//!
//! Field names follow the proto3 JSON mapping (camelCase, or the proto
//! names); 64-bit integers are ints; `bytes` fields are base64 text.
//! Invalid schemas and data are error values.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;

use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, SerializeOptions};

use crate::runtime::value::{ModuleValue, Value};

use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef};
use crate::native::util::{bytes_of, bytes_value};
use crate::native::util::{list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // load(path | [paths], { includes: [dirs] }) — imports are searched in `includes` (default: the files' folders)
    e.insert("load".into(), nfn_var("load", 1, |a| {
        max_args(a, 2, "protobuf.load")?;
        let files: Vec<String> = match &a[0] {
            Value::Str(s) => vec![s.to_string()],
            Value::List(l) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
            other => return Err(format!("protobuf.load: expects a path or a list of paths, got '{}'", other.type_name())),
        };
        let mut includes: Vec<String> = match opt_arg(a, 1) {
            Some(Value::Map(m)) => match m.borrow().get("includes") {
                Some(Value::List(l)) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
                Some(Value::Str(s)) => vec![s.to_string()],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        if includes.is_empty() {
            for f in &files {
                let dir = std::path::Path::new(f).parent().map(|p| p.to_string_lossy().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into());
                if !includes.contains(&dir) {
                    includes.push(dir);
                }
            }
        }
        let mut c = protox::Compiler::new(&includes).map_err(|e| io_err!("protobuf.load: {}", e))?;
        c.include_imports(true);
        c.open_files(&files).map_err(|e| io_err!("protobuf.load: {}", e))?;
        Ok(schema(c.descriptor_pool(), c.encode_file_descriptor_set()))
    }));
    // parse(source, { name = "input.proto", imports: { "other.proto": source } })
    e.insert("parse".into(), nfn_var("parse", 1, |a| {
        max_args(a, 2, "protobuf.parse")?;
        let src = str_arg(a, 0, "protobuf.parse")?;
        let o = match opt_arg(a, 1) {
            Some(Value::Map(m)) => m.borrow().clone(),
            _ => HashMap::new(),
        };
        let name = o.get("name").map(|v| v.to_display_string()).unwrap_or_else(|| "input.proto".into());
        let mut sources = HashMap::new();
        if let Some(Value::Map(imp)) = o.get("imports") {
            for (k, v) in imp.borrow().iter() {
                sources.insert(k.clone(), v.to_display_string());
            }
        }
        sources.insert(name.clone(), src);
        let mut chain = protox::file::ChainFileResolver::new();
        chain.add(MemResolver(sources));
        chain.add(protox::file::GoogleFileResolver::new());
        let mut c = protox::Compiler::with_file_resolver(chain);
        c.include_imports(true);
        c.open_file(&name).map_err(|e| io_err!("protobuf.parse: {}", e))?;
        Ok(schema(c.descriptor_pool(), c.encode_file_descriptor_set()))
    }));
    // loadDescriptor(path.pb | bytes) — a FileDescriptorSet (protoc --descriptor_set_out, buf build)
    e.insert("loadDescriptor".into(), nfn_var("loadDescriptor", 1, |a| {
        max_args(a, 1, "protobuf.loadDescriptor")?;
        let bytes = match &a[0] {
            Value::Str(p) => std::fs::read(p).map_err(|e| io_err!("protobuf.loadDescriptor: {}: {}", p, e))?,
            other => bytes_of(other, "protobuf.loadDescriptor")?,
        };
        let pool = DescriptorPool::decode(bytes.as_slice()).map_err(|e| io_err!("protobuf.loadDescriptor: {}", e))?;
        Ok(schema(pool, bytes))
    }));
    ModuleValue {
        name: "fogu:protobuf".into(),
        exports: RefCell::new(e),
    }
}

struct MemResolver(HashMap<String, String>);

/// Compiles `.proto` sources held in memory (name → text), starting at `root`
/// (for modules that embed their protocol, like `fogu:etcd`).
pub(crate) fn compile_sources(sources: HashMap<String, String>, root: &str) -> Result<DescriptorPool, String> {
    let mut chain = protox::file::ChainFileResolver::new();
    chain.add(MemResolver(sources));
    chain.add(protox::file::GoogleFileResolver::new());
    let mut c = protox::Compiler::with_file_resolver(chain);
    c.include_imports(true);
    c.open_file(root).map_err(|e| format!("protobuf: {}", e))?;
    Ok(c.descriptor_pool())
}

impl protox::file::FileResolver for MemResolver {
    fn open_file(&self, name: &str) -> Result<protox::file::File, protox::Error> {
        match self.0.get(name) {
            Some(src) => protox::file::File::from_source(name, src),
            None => Err(protox::Error::file_not_found(name)),
        }
    }
}

fn schema(pool: DescriptorPool, set: Vec<u8>) -> Value {
    NativeObjectRef::new(Schema { pool, set }).to_value()
}

pub struct Schema {
    pool: DescriptorPool,
    set: Vec<u8>,
}

fn kind_name(f: &prost_reflect::FieldDescriptor) -> String {
    use prost_reflect::Kind::*;
    match f.kind() {
        Double => "double".into(),
        Float => "float".into(),
        Int32 => "int32".into(),
        Int64 => "int64".into(),
        Uint32 => "uint32".into(),
        Uint64 => "uint64".into(),
        Sint32 => "sint32".into(),
        Sint64 => "sint64".into(),
        Fixed32 => "fixed32".into(),
        Fixed64 => "fixed64".into(),
        Sfixed32 => "sfixed32".into(),
        Sfixed64 => "sfixed64".into(),
        Bool => "bool".into(),
        String => "string".into(),
        Bytes => "bytes".into(),
        Message(m) => m.full_name().to_string(),
        Enum(e) => e.full_name().to_string(),
    }
}

impl Schema {
    fn message(&self, name: &str) -> Result<MessageDescriptor, String> {
        self.pool.get_message_by_name(name.trim_start_matches('.')).ok_or_else(|| {
            let known: Vec<String> = self.pool.all_messages().map(|m| m.full_name().to_string()).filter(|n| !n.starts_with("google.protobuf.")).collect();
            format!("protobuf: no message '{}' (messages: {})", name, known.join(", "))
        })
    }

    fn from_value(&self, desc: MessageDescriptor, v: &Value) -> Result<DynamicMessage, String> {
        message_from_value(desc, v)
    }

    fn to_value(&self, m: &DynamicMessage, o: Option<&Value>) -> Result<Value, String> {
        message_to_value(m, o)
    }
}

/// The descriptors of a `ProtoSchema` value (for fogu:grpc).
pub(crate) fn pool_of(v: &Value) -> Result<DescriptorPool, String> {
    if let Value::Object(o) = v {
        if let Some(s) = o.inner().borrow().as_any().downcast_ref::<Schema>() {
            return Ok(s.pool.clone());
        }
    }
    Err(format!("expected a schema from protobuf.load/parse, got '{}'", v.type_name()))
}

/// A Fogu map → a message (the proto3 JSON mapping).
pub(crate) fn message_from_value(desc: MessageDescriptor, v: &Value) -> Result<DynamicMessage, String> {
    let json = stringify_value(v);
    let mut de = serde_json::Deserializer::from_str(&json);
    let m = DynamicMessage::deserialize(desc, &mut de).map_err(|e| io_err!("protobuf: {}", e))?;
    de.end().map_err(|e| io_err!("protobuf: {}", e))?;
    Ok(m)
}

/// A message → a Fogu map; `o` holds the `decode` options (defaults, protoNames, enumNumbers).
pub(crate) fn message_to_value(m: &DynamicMessage, o: Option<&Value>) -> Result<Value, String> {
    let flag = |k: &str| matches!(o, Some(Value::Map(mm)) if mm.borrow().get(k).is_some_and(Value::is_truthy));
    let opts = SerializeOptions::new()
        .stringify_64_bit_integers(false)
        .use_proto_field_name(flag("protoNames"))
        .use_enum_numbers(flag("enumNumbers"))
        .skip_default_fields(!flag("defaults"));
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::new(&mut out);
    m.serialize_with_options(&mut ser, &opts).map_err(|e| io_err!("protobuf: {}", e))?;
    parse_value(&String::from_utf8_lossy(&out)).map_err(|e| io_err!("protobuf: {}", e))
}

/// Encodes a message to bytes.
pub(crate) fn encode_message(m: &DynamicMessage) -> Vec<u8> {
    m.encode_to_vec()
}

/// Decodes bytes as a message of type `desc`.
pub(crate) fn decode_message(desc: MessageDescriptor, data: &[u8]) -> Result<DynamicMessage, String> {
    DynamicMessage::decode(desc, data).map_err(|e| io_err!("protobuf: cannot decode {}: {}", "message", e))
}

impl FoguObject for Schema {
    fn name(&self) -> &str {
        "ProtoSchema"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "messages" => crate::native::util::str_list(self.pool.all_messages().map(|m| m.full_name().to_string()).filter(|n| !n.starts_with("google.protobuf."))),
            "enums" => crate::native::util::str_list(self.pool.all_enums().map(|m| m.full_name().to_string()).filter(|n| !n.starts_with("google.protobuf."))),
            "services" => list(
                self.pool
                    .services()
                    .map(|s| {
                        map(vec![
                            ("name", Value::Str(s.full_name().to_string().into())),
                            (
                                "methods",
                                list(
                                    s.methods()
                                        .map(|m| {
                                            map(vec![
                                                ("name", Value::Str(m.name().to_string().into())),
                                                ("input", Value::Str(m.input().full_name().to_string().into())),
                                                ("output", Value::Str(m.output().full_name().to_string().into())),
                                                ("clientStreaming", Value::Bool(m.is_client_streaming())),
                                                ("serverStreaming", Value::Bool(m.is_server_streaming())),
                                            ])
                                        })
                                        .collect(),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
            "__display__" => Value::Str(format!("<ProtoSchema messages={}>", self.pool.all_messages().filter(|m| !m.full_name().starts_with("google.protobuf.")).count()).into()),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, a: &[Value]) -> Result<Value, String> {
        match method {
            // encode(type, map) → bytes
            "encode" => {
                max_args(a, 2, "encode")?;
                let desc = self.message(&str_arg(a, 0, "encode")?)?;
                let m = self.from_value(desc, a.get(1).unwrap_or(&Value::Nil))?;
                Ok(bytes_value(m.encode_to_vec()))
            }
            // decode(type, bytes, { defaults, protoNames, enumNumbers }) → map
            "decode" => {
                max_args(a, 3, "decode")?;
                let desc = self.message(&str_arg(a, 0, "decode")?)?;
                let data = bytes_of(a.get(1).unwrap_or(&Value::Nil), "decode")?;
                let m = DynamicMessage::decode(desc, data.as_slice()).map_err(|e| io_err!("protobuf: cannot decode: {}", e))?;
                self.to_value(&m, a.get(2))
            }
            // toJson(type, bytes) → str (proto3 JSON) · fromJson(type, str) → bytes
            "toJson" => {
                let desc = self.message(&str_arg(a, 0, "toJson")?)?;
                let data = bytes_of(a.get(1).unwrap_or(&Value::Nil), "toJson")?;
                let m = DynamicMessage::decode(desc, data.as_slice()).map_err(|e| io_err!("protobuf: cannot decode: {}", e))?;
                serde_json::to_string(&m).map(Value::str).map_err(|e| io_err!("protobuf: {}", e))
            }
            "fromJson" => {
                let desc = self.message(&str_arg(a, 0, "fromJson")?)?;
                let text = str_arg(a, 1, "fromJson")?;
                let mut de = serde_json::Deserializer::from_str(&text);
                let m = DynamicMessage::deserialize(desc, &mut de).map_err(|e| io_err!("protobuf: {}", e))?;
                Ok(bytes_value(m.encode_to_vec()))
            }
            // validate(type, map) → true or error value
            "validate" => {
                let desc = self.message(&str_arg(a, 0, "validate")?)?;
                self.from_value(desc, a.get(1).unwrap_or(&Value::Nil)).map(|_| Value::Bool(true))
            }
            // fields(type) → [{ name, jsonName, number, type, repeated, map, optional }]
            "fields" => {
                let desc = self.message(&str_arg(a, 0, "fields")?)?;
                Ok(list(
                    desc.fields()
                        .map(|f| {
                            map(vec![
                                ("name", Value::Str(f.name().to_string().into())),
                                ("jsonName", Value::Str(f.json_name().to_string().into())),
                                ("number", Value::Int(f.number() as i64)),
                                ("type", Value::Str(kind_name(&f).into())),
                                ("repeated", Value::Bool(f.is_list())),
                                ("map", Value::Bool(f.is_map())),
                            ])
                        })
                        .collect(),
                ))
            }
            // enumValues(type) → { NAME: number }
            "enumValues" => {
                let name = str_arg(a, 0, "enumValues")?;
                let e = self.pool.get_enum_by_name(&name).ok_or_else(|| format!("protobuf: no enum '{}'", name))?;
                Ok(map(e.values().map(|v| (v.name().to_string(), Value::Int(v.number() as i64))).collect::<Vec<_>>().iter().map(|(k, v)| (k.as_str(), v.clone())).collect()))
            }
            // descriptor() → bytes of the FileDescriptorSet · saveDescriptor(path)
            "descriptor" => Ok(bytes_value(self.set.clone())),
            "saveDescriptor" => {
                let path = str_arg(a, 0, "saveDescriptor")?;
                std::fs::write(&path, &self.set).map_err(|e| io_err!("protobuf.saveDescriptor: {}: {}", path, e))?;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on ProtoSchema ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["encode", "decode", "toJson", "fromJson", "validate", "fields", "enumValues", "descriptor", "saveDescriptor"]
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
