//! `fogu:grpc` module — gRPC servers and clients without code generation:
//! the messages are described by a `fogu:protobuf` schema (`.proto` files
//! compiled at run time) and travel as Fogu maps.
//!
//! ```fogu
//! import grpc from "fogu:grpc"
//! import protobuf from "fogu:protobuf"
//!
//! const schema = protobuf.load("greeter.proto")?
//!
//! // server: one handler per method; each request runs as its own `fg` task
//! const srv = grpc.server(schema)
//! srv.handle("demo.Greeter/SayHello", fn(req, call) { return { message: "Hello, " + req.name } })
//! srv.handle("Count", fn(req, call) {                       // server streaming
//!     for i in 0..req.n { call.send({ n: i }) }
//! })
//! srv.handle("Chat", fn(msgs, call) {                       // bidirectional
//!     for m in msgs { call.send({ text: "echo " + m.text }) }
//! })
//! srv.listen(50051)?
//! fg srv.serve()                                            // until srv.close()
//!
//! // client
//! const c = grpc.connect("http://localhost:50051", schema)?
//! print(c.call("SayHello", { name: "Ana" })?)               // { message: Hello, Ana }
//! print(c.call("Count", { n: 3 })?)                         // a list for server streaming
//! for m in c.stream("Count", { n: 3 })? { print(m) }        // or one message at a time
//! const chat = c.stream("Chat")?                            // requests one at a time too
//! chat.send({ text: "hi" })
//! print(chat.next()?)                                       // { text: echo hi }
//! chat.end()
//! ```
//!
//! - Methods are named `"pkg.Service/Method"`, `"pkg.Service.Method"` or
//!   just `"Method"` when it is unique in the schema.
//! - Server-streaming methods answer with `call.send(msg)` (or by returning a
//!   list or a stream). The handler of a client-streaming or bidirectional
//!   method gets its requests as a `GrpcStream`, while they arrive: `for m in
//!   msgs`, `msgs.next()`, `stream.from(msgs)`, `msgs.toList()`.
//! - On the client, `call` takes a list of requests for those methods;
//!   `stream(method)` without one opens the call: `send(msg)` each request,
//!   `end()` when there are no more, and `next()` / `for` the responses. A
//!   `GrpcStream` is a `fogu:stream` source, and `pipe` sends into it.
//! - A handler fails the call by returning an error: `return nil,
//!   grpc.status("NOT_FOUND", "no such user")` (or `error("NOT_FOUND: ...")`);
//!   any other error is `UNKNOWN`. On the client a failed call is an error
//!   value `"grpc: NOT_FOUND: no such user"`. `srv.use(fn(call))` runs before
//!   every handler and fails the call the same way (authentication).
//! - The server answers `grpc.health.v1.Health/Check` (`srv.health(service,
//!   status)`; off with `{ health: false }`) and, with `{ reflection: true }`,
//!   server reflection (`grpcurl list`).
//! - `https://` URLs use TLS (system roots, or `{ ca: "ca.pem" }`, and `{ cert,
//!   key }` for a client certificate); `listen(port, { cert, key })` serves
//!   TLS, and `clientCa` makes clients prove who they are (mTLS). Metadata
//!   travels in `{ metadata: { key: value } }` and arrives as `call.metadata`.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use ahash::AHashMap as HashMap;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Buf, BufMut};
use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::{Code, Status};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched::{self, IoGuard};
use crate::runtime::value::Str;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::protobuf::{decode_message, encode_message, message_from_value, message_to_value, pool_of};
use crate::native::util::{duration_arg, list, max_args, nfn_var, opt_arg, str_arg};

/// gRPC status codes, by number.
const CODES: &[&str] = &[
    "OK", "CANCELLED", "UNKNOWN", "INVALID_ARGUMENT", "DEADLINE_EXCEEDED", "NOT_FOUND", "ALREADY_EXISTS",
    "PERMISSION_DENIED", "RESOURCE_EXHAUSTED", "FAILED_PRECONDITION", "ABORTED", "OUT_OF_RANGE", "UNIMPLEMENTED",
    "INTERNAL", "UNAVAILABLE", "DATA_LOSS", "UNAUTHENTICATED",
];

fn code_name(c: Code) -> &'static str {
    CODES.get(c as usize).copied().unwrap_or("UNKNOWN")
}

/// A handler's error message → a status: `"NOT_FOUND: no user"` keeps its code.
fn status_of(msg: &str) -> Status {
    let msg = msg.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).unwrap_or(msg);
    if let Some((code, rest)) = msg.split_once(": ") {
        if let Some(i) = CODES.iter().position(|c| *c == code) {
            return Status::new(Code::from_i32(i as i32), rest);
        }
    }
    Status::unknown(msg)
}

/// A failed call, as an error value for Fogu.
fn status_err(s: &Status) -> String {
    io_err!("grpc: {}: {}", code_name(s.code()), s.message())
}

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    // server(schema, { health, reflection }) → GrpcServer
    e.insert("server".into(), nfn_var("server", 1, new_server));
    // connect(url, schema, { timeout, metadata, ca }) → GrpcClient (connects on the first call)
    e.insert("connect".into(), nfn_var("connect", 2, connect));
    // status(code, message) → an error value a handler returns to fail the call
    e.insert("status".into(), nfn_var("status", 2, |a| {
        max_args(a, 2, "grpc.status")?;
        let code = str_arg(a, 0, "grpc.status")?.to_ascii_uppercase();
        if !CODES.contains(&code.as_str()) {
            return Err(format!("grpc.status: unknown code '{}' ({})", code, CODES.join(" ")));
        }
        Err(io_err!("{}: {}", code, str_arg(a, 1, "grpc.status")?))
    }));
    e.insert("codes".into(), list(CODES.iter().map(|c| Value::Str(c.to_string().into())).collect()));
    ModuleValue {
        name: "fogu:grpc".into(),
        exports: RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// methods and messages
// ---------------------------------------------------------------------

#[derive(Clone)]
struct Method {
    /// `/pkg.Service/Method`
    path: String,
    input: MessageDescriptor,
    output: MessageDescriptor,
    client_streaming: bool,
    server_streaming: bool,
}

/// `"pkg.Service/Method"`, `"pkg.Service.Method"` or a unique `"Method"`.
fn find_method(pool: &DescriptorPool, name: &str) -> Result<Method, String> {
    let name = name.trim().trim_start_matches('/');
    let split = name.split_once('/').or_else(|| name.rsplit_once('.'));
    let found: Vec<prost_reflect::MethodDescriptor> = match split {
        Some((svc, m)) => pool.get_service_by_name(svc).and_then(|s| s.methods().find(|x| x.name() == m)).into_iter().collect(),
        None => pool.services().flat_map(|s| s.methods().filter(|x| x.name() == name).collect::<Vec<_>>()).collect(),
    };
    match found.as_slice() {
        [m] => Ok(Method {
            path: format!("/{}/{}", m.parent_service().full_name(), m.name()),
            input: m.input(),
            output: m.output(),
            client_streaming: m.is_client_streaming(),
            server_streaming: m.is_server_streaming(),
        }),
        [] => {
            let all: Vec<String> = pool.services().flat_map(|s| s.methods().map(|m| format!("{}/{}", s.full_name(), m.name())).collect::<Vec<_>>()).collect();
            Err(format!("grpc: no method '{}' in the schema ({})", name, all.join(", ")))
        }
        _ => Err(format!("grpc: method '{}' exists in several services: use \"pkg.Service/{}\"", name, name)),
    }
}

fn encode(desc: &MessageDescriptor, v: &Value) -> Result<Vec<u8>, String> {
    Ok(encode_message(&message_from_value(desc.clone(), v)?))
}

/// Decodes with every field present (proto3 defaults included), so handlers can read `req.name` safely.
fn decode(desc: &MessageDescriptor, data: &[u8]) -> Result<Value, String> {
    thread_local! {
        static DEFAULTS: Value = crate::native::util::map(vec![("defaults", Value::Bool(true))]);
    }
    let m = decode_message(desc.clone(), data)?;
    DEFAULTS.with(|o| message_to_value(&m, Some(o)))
}

/// tonic only frames the messages: they are encoded/decoded on the interpreter thread.
#[derive(Clone, Copy, Default)]
pub(crate) struct RawCodec;
pub(crate) struct RawEncoder;
pub(crate) struct RawDecoder;

impl Codec for RawCodec {
    type Encode = Vec<u8>;
    type Decode = Vec<u8>;
    type Encoder = RawEncoder;
    type Decoder = RawDecoder;
    fn encoder(&mut self) -> RawEncoder {
        RawEncoder
    }
    fn decoder(&mut self) -> RawDecoder {
        RawDecoder
    }
}

impl Encoder for RawEncoder {
    type Item = Vec<u8>;
    type Error = Status;
    fn encode(&mut self, item: Vec<u8>, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        dst.put_slice(&item);
        Ok(())
    }
}

impl Decoder for RawDecoder {
    type Item = Vec<u8>;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Vec<u8>>, Status> {
        let mut v = vec![0; src.remaining()];
        src.copy_to_slice(&mut v);
        Ok(Some(v))
    }
}

/// Request metadata as (key, value) pairs, without the protocol's own headers.
fn metadata_pairs(h: &http::HeaderMap) -> Vec<(String, String)> {
    h.iter()
        .filter(|(k, _)| {
            let k = k.as_str();
            !(k.starts_with("grpc-") || k == "content-type" || k == "te" || k.starts_with(':'))
        })
        .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect()
}

fn metadata_value(pairs: &[(String, String)]) -> Value {
    let m: HashMap<Str, Value> = pairs.iter().map(|(k, v)| (k.as_str().into(), Value::Str(v.clone().into()))).collect();
    Value::Map(Rc::new(RefCell::new(m.into_iter().collect())))
}

/// `{ metadata: { k: v } }` → pairs (keys lower case, as HTTP/2 wants them).
fn metadata_opt(o: &HashMap<Str, Value>) -> Result<Vec<(String, String)>, String> {
    match o.get("metadata") {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_display_string())).collect()),
        Some(other) => Err(format!("grpc: 'metadata' must be a map, got '{}'", other.type_name())),
    }
}

fn opts_map(v: Option<&Value>, what: &str) -> Result<HashMap<Str, Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

// ---------------------------------------------------------------------
// server
// ---------------------------------------------------------------------

type Reply = tokio::sync::mpsc::UnboundedSender<Result<Vec<u8>, Status>>;

/// A request received by the network side, waiting for its Fogu handler.
struct Job {
    path: String,
    /// The request of a unary or server-streaming method.
    messages: Vec<Vec<u8>>,
    /// The requests of a client-streaming or bidirectional method, still arriving.
    incoming: Option<tonic::Streaming<Vec<u8>>>,
    metadata: Vec<(String, String)>,
    peer: String,
    reply: Reply,
}

#[derive(Default)]
struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    /// Paths with a handler (others answer UNIMPLEMENTED right away) → whether
    /// the client streams its requests.
    known: Mutex<HashMap<String, bool>>,
    closed: AtomicBool,
    /// The schema, for server reflection.
    pool: DescriptorPool,
    /// `grpc.health.v1.Health/Check` (on unless `{ health: false }`) and the
    /// statuses set with `srv.health(service, status)`.
    health_on: AtomicBool,
    health: Mutex<HashMap<String, i32>>,
    /// `grpc.reflection.v1` (and v1alpha), with `{ reflection: true }`.
    reflection: AtomicBool,
}

pub struct GrpcServer {
    pool: DescriptorPool,
    handlers: HashMap<String, (Value, Method)>,
    /// `srv.use(fn(call))`: run before every handler, in order.
    interceptors: Vec<Value>,
    shared: Arc<Shared>,
    task: Option<tokio::task::JoinHandle<()>>,
    port: Option<u16>,
    host: String,
    tls: bool,
    serving: bool,
}

fn new_server(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "grpc.server")?;
    let pool = pool_of(&a[0]).map_err(|e| format!("grpc.server: {}", e))?;
    let o = opts_map(opt_arg(a, 1), "grpc.server")?;
    let flag = |k: &str, default: bool| o.get(k).map_or(default, Value::is_truthy);
    let shared = Shared {
        pool: pool.clone(),
        health_on: AtomicBool::new(flag("health", true)),
        reflection: AtomicBool::new(flag("reflection", false)),
        ..Default::default()
    };
    Ok(NativeObjectRef::new(GrpcServer {
        pool,
        handlers: HashMap::new(),
        interceptors: Vec::new(),
        shared: Arc::new(shared),
        task: None,
        port: None,
        host: String::new(),
        tls: false,
        serving: false,
    })
    .to_value())
}

fn is_function(f: &Value) -> bool {
    matches!(f, Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_))
}

impl GrpcServer {
    fn add(&mut self, name: &str, f: Value) -> Result<(), String> {
        if !is_function(&f) {
            return Err(format!("handle: the handler of '{}' must be a function, got '{}'", name, f.type_name()));
        }
        let m = find_method(&self.pool, name)?;
        self.shared.known.lock().unwrap_or_else(|e| e.into_inner()).insert(m.path.clone(), m.client_streaming);
        self.handlers.insert(m.path.clone(), (f, m));
        Ok(())
    }

    fn listen(&mut self, args: &[Value]) -> Result<Value, String> {
        max_args(args, 2, "listen")?;
        if self.task.is_some() {
            return Err("listen: the server is already listening".into());
        }
        let addr = match args.first() {
            Some(Value::Int(p)) => format!("127.0.0.1:{}", p), // like app.listen: the network only when asked ("0.0.0.0:port")
            Some(Value::Str(s)) if s.starts_with(':') => format!("127.0.0.1{}", s),
            Some(Value::Str(s)) => s.to_string(),
            _ => return Err("listen(port | \"host:port\", { cert, key, clientCa })".into()),
        };
        let o = opts_map(args.get(1), "listen")?;
        let client_ca = o.get("clientCa").filter(|v| !matches!(v, Value::Nil)).map(Value::to_display_string);
        let tls = match (o.get("cert"), o.get("key")) {
            (Some(c), Some(k)) => Some(tls_acceptor(&c.to_display_string(), &k.to_display_string(), client_ca.as_deref())?),
            (None, None) if client_ca.is_some() => return Err("listen: 'clientCa' needs TLS ('cert' and 'key')".into()),
            (None, None) => None,
            _ => return Err("listen: TLS needs both 'cert' and 'key'".into()),
        };
        let std_listener = std::net::TcpListener::bind(&addr).map_err(|e| io_err!("grpc: cannot listen on {}: {}", addr, e))?;
        std_listener.set_nonblocking(true).map_err(|e| io_err!("grpc: {}", e))?;
        let local = std_listener.local_addr().map_err(|e| io_err!("grpc: {}", e))?;
        let shared = Arc::clone(&self.shared);
        self.tls = tls.is_some();
        let rt = crate::native::rt::tokio();
        let listener = {
            let _g = rt.enter();
            tokio::net::TcpListener::from_std(std_listener).map_err(|e| io_err!("grpc: {}", e))?
        };
        self.task = Some(rt.spawn(accept_loop(listener, shared, tls)));
        self.port = Some(local.port());
        self.host = local.ip().to_string();
        Ok(Value::Int(local.port() as i64))
    }

    fn close(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        if let Some(t) = self.task.take() {
            t.abort();
        }
        // pending requests get UNAVAILABLE
        for job in self.shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            let _ = job.reply.send(Err(Status::unavailable("the server is closing")));
        }
        sched::wake();
    }
}

impl Drop for GrpcServer {
    fn drop(&mut self) {
        self.close();
    }
}

fn read_pem_certs(path: &str, what: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    let data = std::fs::read(path).map_err(|e| io_err!("grpc: {} {}: {}", what, path, e))?;
    <rustls::pki_types::CertificateDer as rustls::pki_types::pem::PemObject>::pem_slice_iter(&data)
        .collect::<Result<_, _>>()
        .map_err(|e| io_err!("grpc: {} {}: {}", what, path, e))
}

/// TLS for a server; with `client_ca`, clients must present a certificate it signed (mTLS).
fn tls_acceptor(cert: &str, key: &str, client_ca: Option<&str>) -> Result<tokio_rustls::TlsAcceptor, String> {
    let certs = read_pem_certs(cert, "cert")?;
    let key = {
        let data = std::fs::read(key).map_err(|e| io_err!("grpc: key {}: {}", key, e))?;
        <rustls::pki_types::PrivateKeyDer as rustls::pki_types::pem::PemObject>::from_pem_slice(&data).map_err(|e| match e {
            rustls::pki_types::pem::Error::NoItemsFound => io_err!("grpc: no private key in {}", key),
            e => io_err!("grpc: key {}: {}", key, e),
        })?
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("grpc: TLS: {}", e))?;
    let builder = match client_ca {
        None => builder.with_no_client_auth(),
        Some(ca) => {
            let mut roots = rustls::RootCertStore::empty();
            for c in read_pem_certs(ca, "clientCa")? {
                roots.add(c).map_err(|e| io_err!("grpc: clientCa {}: {}", ca, e))?;
            }
            let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .map_err(|e| io_err!("grpc: clientCa {}: {}", ca, e))?;
            builder.with_client_cert_verifier(verifier)
        }
    };
    let mut cfg = builder.with_single_cert(certs, key).map_err(|e| io_err!("grpc: TLS: {}", e))?;
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(cfg)))
}

async fn accept_loop(listener: tokio::net::TcpListener, shared: Arc<Shared>, tls: Option<tokio_rustls::TlsAcceptor>) {
    loop {
        let Ok((tcp, peer)) = listener.accept().await else { continue };
        let _ = tcp.set_nodelay(true);
        let shared = Arc::clone(&shared);
        let tls = tls.clone();
        tokio::spawn(async move {
            let peer = peer.to_string();
            match tls {
                None => serve_conn(tcp, shared, peer).await,
                Some(acc) => {
                    if let Ok(s) = acc.accept(tcp).await {
                        serve_conn(s, shared, peer).await
                    }
                }
            }
        });
    }
}

async fn serve_conn<IO>(io: IO, shared: Arc<Shared>, peer: String)
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = hyper::service::service_fn(move |req: http::Request<hyper::body::Incoming>| {
        let shared = Arc::clone(&shared);
        let peer = peer.clone();
        async move { Ok::<_, std::convert::Infallible>(dispatch(shared, peer, req).await) }
    });
    let _ = hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(hyper_util::rt::TokioIo::new(io), svc)
        .await;
}

async fn dispatch(shared: Arc<Shared>, peer: String, req: http::Request<hyper::body::Incoming>) -> http::Response<tonic::body::Body> {
    let path = req.uri().path().to_string();
    if shared.closed.load(Ordering::SeqCst) {
        return Status::unavailable("the server is closing").into_http();
    }
    let client_streaming = shared.known.lock().unwrap_or_else(|e| e.into_inner()).get(&path).copied();
    let Some(client_streaming) = client_streaming else {
        // health checking and reflection answer from here, without Fogu code
        if let Some(f) = builtin(&shared, &path) {
            return tonic::server::Grpc::new(RawCodec).streaming(Builtin { shared, f }, req).await;
        }
        return Status::unimplemented(format!("unknown method {}", path)).into_http();
    };
    // every kind of RPC is a stream of messages each way on the wire
    tonic::server::Grpc::new(RawCodec).streaming(Handler { shared, path, peer, client_streaming }, req).await
}

#[derive(Clone)]
struct Handler {
    shared: Arc<Shared>,
    path: String,
    peer: String,
    client_streaming: bool,
}

type ReplyStream = tokio_stream::wrappers::UnboundedReceiverStream<Result<Vec<u8>, Status>>;

impl tonic::codegen::Service<tonic::Request<tonic::Streaming<Vec<u8>>>> for Handler {
    type Response = tonic::Response<ReplyStream>;
    type Error = Status;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Status>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Status>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: tonic::Request<tonic::Streaming<Vec<u8>>>) -> Self::Future {
        let me = self.clone();
        Box::pin(async move {
            let metadata = metadata_pairs(&req.metadata().clone().into_headers());
            let mut stream = req.into_inner();
            // a streaming client's requests reach the handler as they arrive; the
            // others are read first (one message)
            let (messages, incoming) = if me.client_streaming {
                (Vec::new(), Some(stream))
            } else {
                let mut messages = Vec::new();
                while let Some(m) = stream.message().await? {
                    messages.push(m);
                }
                (messages, None)
            };
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            me.shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).push_back(Job {
                path: me.path,
                messages,
                incoming,
                metadata,
                peer: me.peer,
                reply: tx,
            });
            sched::wake();
            Ok(tonic::Response::new(ReplyStream::new(rx)))
        })
    }
}

// ---------------------------------------------------------------------
// health checking and server reflection
// ---------------------------------------------------------------------

const HEALTH_PROTO: &str = r#"syntax = "proto3";
package grpc.health.v1;
message HealthCheckRequest { string service = 1; }
message HealthCheckResponse {
  enum ServingStatus { UNKNOWN = 0; SERVING = 1; NOT_SERVING = 2; SERVICE_UNKNOWN = 3; }
  ServingStatus status = 1;
}
service Health {
  rpc Check(HealthCheckRequest) returns (HealthCheckResponse);
  rpc Watch(HealthCheckRequest) returns (stream HealthCheckResponse);
}
"#;

const REFLECTION_PROTO: &str = r#"syntax = "proto3";
package grpc.reflection.v1;
message ServerReflectionRequest {
  string host = 1;
  oneof message_request {
    string file_by_filename = 3;
    string file_containing_symbol = 4;
    ExtensionRequest file_containing_extension = 5;
    string all_extension_numbers_of_type = 6;
    string list_services = 7;
  }
}
message ExtensionRequest { string containing_type = 1; int32 extension_number = 2; }
message ServerReflectionResponse {
  string valid_host = 1;
  ServerReflectionRequest original_request = 2;
  oneof message_response {
    FileDescriptorResponse file_descriptor_response = 4;
    ExtensionNumberResponse all_extension_numbers_response = 5;
    ListServiceResponse list_services_response = 6;
    ErrorResponse error_response = 7;
  }
}
message FileDescriptorResponse { repeated bytes file_descriptor_proto = 1; }
message ExtensionNumberResponse { string base_type_name = 1; repeated int32 extension_number = 2; }
message ListServiceResponse { repeated ServiceResponse service = 1; }
message ServiceResponse { string name = 1; }
message ErrorResponse { int32 error_code = 1; string error_message = 2; }
service ServerReflection {
  rpc ServerReflectionInfo(stream ServerReflectionRequest) returns (stream ServerReflectionResponse);
}
"#;

const HEALTH_SERVICE: &str = "grpc.health.v1.Health";
const REFLECTION_SERVICE: &str = "grpc.reflection.v1.ServerReflection";

/// The protocols of health checking and reflection, compiled once.
fn builtin_pool() -> &'static DescriptorPool {
    static POOL: std::sync::OnceLock<DescriptorPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let mut src = HashMap::new();
        src.insert("grpc/health/v1/health.proto".to_string(), HEALTH_PROTO.to_string());
        src.insert("grpc/reflection/v1/reflection.proto".to_string(), REFLECTION_PROTO.to_string());
        src.insert(
            "fogu/grpc_builtin.proto".to_string(),
            "syntax = \"proto3\";\nimport \"grpc/health/v1/health.proto\";\nimport \"grpc/reflection/v1/reflection.proto\";\n".to_string(),
        );
        crate::native::protobuf::compile_sources(src, "fogu/grpc_builtin.proto").expect("the built-in gRPC protocols compile")
    })
}

fn builtin_message(name: &str) -> MessageDescriptor {
    builtin_pool().get_message_by_name(name).expect("a built-in gRPC message")
}

type BuiltinFn = fn(&Shared, &[u8]) -> Result<Vec<u8>, Status>;

/// A method the server answers itself, unless the program handles it.
fn builtin(shared: &Shared, path: &str) -> Option<BuiltinFn> {
    match path {
        "/grpc.health.v1.Health/Check" if shared.health_on.load(Ordering::Relaxed) => Some(health_check),
        "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo" | "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
            if shared.reflection.load(Ordering::Relaxed) =>
        {
            Some(reflect)
        }
        _ => None,
    }
}

/// Whether a handler serves a method of `service` ("pkg.Service").
fn serves(shared: &Shared, service: &str) -> bool {
    shared
        .known
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .any(|p| p.strip_prefix('/').and_then(|r| r.rsplit_once('/')).is_some_and(|(s, _)| s == service))
}

fn bad_request(e: String) -> Status {
    Status::invalid_argument(e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX).to_string())
}

/// `grpc.health.v1.Health/Check`: SERVING for "" and every service with a
/// handler, unless `srv.health(service, status)` said otherwise.
fn health_check(shared: &Shared, data: &[u8]) -> Result<Vec<u8>, Status> {
    let req = decode_message(builtin_message("grpc.health.v1.HealthCheckRequest"), data).map_err(bad_request)?;
    let service = req.get_field_by_name("service").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let set = shared.health.lock().unwrap_or_else(|e| e.into_inner()).get(&service).copied();
    let status = match set {
        Some(s) => s,
        None if service.is_empty() || serves(shared, &service) => 1,
        None => return Err(Status::not_found(format!("unknown service {}", service))),
    };
    let mut resp = DynamicMessage::new(builtin_message("grpc.health.v1.HealthCheckResponse"));
    resp.set_field_by_name("status", prost_reflect::Value::EnumNumber(status));
    Ok(encode_message(&resp))
}

/// The file that declares a service, method, message or enum.
fn symbol_file(pool: &DescriptorPool, sym: &str) -> Option<prost_reflect::FileDescriptor> {
    let sym = sym.trim_start_matches('.');
    pool.get_service_by_name(sym)
        .map(|s| s.parent_file())
        .or_else(|| pool.get_message_by_name(sym).map(|m| m.parent_file()))
        .or_else(|| pool.get_enum_by_name(sym).map(|e| e.parent_file()))
        .or_else(|| sym.rsplit_once('.').and_then(|(svc, _)| pool.get_service_by_name(svc)).map(|s| s.parent_file()))
}

/// A file and every file it imports, encoded, the file first.
fn with_imports(fd: prost_reflect::FileDescriptor) -> Vec<prost_reflect::Value> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut todo = vec![fd];
    while let Some(f) = todo.pop() {
        if !seen.insert(f.name().to_string()) {
            continue;
        }
        out.push(prost_reflect::Value::Bytes(f.file_descriptor_proto().encode_to_vec().into()));
        todo.extend(f.dependencies());
    }
    out
}

/// `grpc.reflection.v1.ServerReflection/ServerReflectionInfo`: one answer per request.
fn reflect(shared: &Shared, data: &[u8]) -> Result<Vec<u8>, Status> {
    use prost_reflect::Value as P;
    let req = decode_message(builtin_message("grpc.reflection.v1.ServerReflectionRequest"), data).map_err(bad_request)?;
    let text = |f: &str| if req.has_field_by_name(f) { req.get_field_by_name(f).and_then(|v| v.as_str().map(str::to_string)) } else { None };
    let mut resp = DynamicMessage::new(builtin_message("grpc.reflection.v1.ServerReflectionResponse"));
    resp.set_field_by_name("valid_host", P::String(text("host").unwrap_or_default()));
    resp.set_field_by_name("original_request", P::Message(req.clone()));
    let pools = [&shared.pool, builtin_pool()];
    let file = if let Some(name) = text("file_by_filename") {
        Some(pools.iter().find_map(|p| p.get_file_by_name(&name)).ok_or_else(|| format!("no file named {}", name)))
    } else {
        text("file_containing_symbol").map(|sym| pools.iter().find_map(|p| symbol_file(p, &sym)).ok_or_else(|| format!("no symbol {}", sym)))
    };
    let error = |code: i32, msg: &str| {
        let mut e = DynamicMessage::new(builtin_message("grpc.reflection.v1.ErrorResponse"));
        e.set_field_by_name("error_code", P::I32(code));
        e.set_field_by_name("error_message", P::String(msg.to_string()));
        P::Message(e)
    };
    match file {
        Some(Ok(fd)) => {
            let mut r = DynamicMessage::new(builtin_message("grpc.reflection.v1.FileDescriptorResponse"));
            r.set_field_by_name("file_descriptor_proto", P::List(with_imports(fd)));
            resp.set_field_by_name("file_descriptor_response", P::Message(r));
        }
        Some(Err(msg)) => resp.set_field_by_name("error_response", error(Code::NotFound as i32, &msg)),
        None if req.has_field_by_name("list_services") => {
            let mut names: Vec<String> = shared.pool.services().map(|s| s.full_name().to_string()).collect();
            if shared.health_on.load(Ordering::Relaxed) {
                names.push(HEALTH_SERVICE.into());
            }
            names.push(REFLECTION_SERVICE.into());
            let services = names
                .into_iter()
                .map(|n| {
                    let mut s = DynamicMessage::new(builtin_message("grpc.reflection.v1.ServiceResponse"));
                    s.set_field_by_name("name", P::String(n));
                    P::Message(s)
                })
                .collect();
            let mut r = DynamicMessage::new(builtin_message("grpc.reflection.v1.ListServiceResponse"));
            r.set_field_by_name("service", P::List(services));
            resp.set_field_by_name("list_services_response", P::Message(r));
        }
        None if req.has_field_by_name("all_extension_numbers_of_type") => {
            // proto3 schemas declare no extensions
            let mut r = DynamicMessage::new(builtin_message("grpc.reflection.v1.ExtensionNumberResponse"));
            r.set_field_by_name("base_type_name", P::String(text("all_extension_numbers_of_type").unwrap_or_default()));
            resp.set_field_by_name("all_extension_numbers_response", P::Message(r));
        }
        None => resp.set_field_by_name("error_response", error(Code::Unimplemented as i32, "extensions are not supported")),
    }
    Ok(encode_message(&resp))
}

#[derive(Clone)]
struct Builtin {
    shared: Arc<Shared>,
    f: BuiltinFn,
}

impl tonic::codegen::Service<tonic::Request<tonic::Streaming<Vec<u8>>>> for Builtin {
    type Response = tonic::Response<ReplyStream>;
    type Error = Status;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Status>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Status>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: tonic::Request<tonic::Streaming<Vec<u8>>>) -> Self::Future {
        let me = self.clone();
        Box::pin(async move {
            let mut stream = req.into_inner();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                while let Ok(Some(m)) = stream.message().await {
                    let r = (me.f)(&me.shared, &m);
                    let failed = r.is_err();
                    if tx.send(r).is_err() || failed {
                        break;
                    }
                }
            });
            Ok(tonic::Response::new(ReplyStream::new(rx)))
        })
    }
}

impl FoguObject for GrpcServer {
    fn name(&self) -> &str {
        "GrpcServer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "port" => Some(self.port.map(|p| Value::Int(p as i64)).unwrap_or(Value::Nil)),
            "address" => Some(match self.port {
                Some(p) => Value::Str(format!("{}://{}:{}", if self.tls { "https" } else { "http" }, self.host, p).into()),
                None => Value::Nil,
            }),
            "methods" => Some(list(self.handlers.keys().map(|k| Value::Str(k.trim_start_matches('/').to_string().into())).collect())),
            "closed" => Some(Value::Bool(self.shared.closed.load(Ordering::SeqCst))),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // handle("pkg.Service/Method", fn(req, call)) · handle({ "Method": fn, ... })
            "handle" => {
                max_args(args, 2, "handle")?;
                match (args.first(), args.get(1)) {
                    (Some(Value::Str(name)), Some(f)) => self.add(name, f.clone())?,
                    (Some(Value::Map(m)), None) => {
                        for (k, f) in m.borrow().iter() {
                            self.add(k, f.clone())?;
                        }
                    }
                    _ => return Err("handle(method, fn) or handle({ method: fn })".into()),
                }
                Ok(Value::Nil)
            }
            // service("pkg.Service", { Method: fn, ... })
            "service" => {
                max_args(args, 2, "service")?;
                let svc = str_arg(args, 0, "service")?;
                let Some(Value::Map(m)) = args.get(1) else { return Err("service(name, { method: fn })".into()) };
                for (k, f) in m.borrow().iter() {
                    self.add(&format!("{}/{}", svc, k), f.clone())?;
                }
                Ok(Value::Nil)
            }
            // use(fn(call)): runs before every handler; returning an error fails the call with it
            "use" => {
                max_args(args, 1, "use")?;
                match args.first() {
                    Some(f) if is_function(f) => self.interceptors.push(f.clone()),
                    other => return Err(format!("use: expects a function, got '{}'", other.map_or("nothing", |v| v.type_name()))),
                }
                Ok(Value::Nil)
            }
            // health(service, "SERVING" | "NOT_SERVING" | "UNKNOWN"): what grpc.health.v1 answers ("" = the server)
            "health" => {
                max_args(args, 2, "health")?;
                let svc = str_arg(args, 0, "health")?;
                let st = str_arg(args, 1, "health")?.to_ascii_uppercase();
                let n = match st.as_str() {
                    "UNKNOWN" => 0,
                    "SERVING" => 1,
                    "NOT_SERVING" => 2,
                    _ => return Err(format!("health: unknown status '{}' (SERVING NOT_SERVING UNKNOWN)", st)),
                };
                self.shared.health.lock().unwrap_or_else(|e| e.into_inner()).insert(svc, n);
                Ok(Value::Nil)
            }
            "listen" => self.listen(args),
            "close" => {
                self.close();
                Ok(Value::Nil)
            }
            "serve" => Err("serve() must be called directly on the server".into()),
            other => Err(format!("method '{}' does not exist on GrpcServer ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["handle", "service", "use", "health", "listen", "serve", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The second argument of a handler: request metadata and, for server
/// streaming, `send(msg)`.
pub struct GrpcCall {
    method: String,
    metadata: Vec<(String, String)>,
    peer: String,
    output: MessageDescriptor,
    streaming: bool,
    reply: Option<Reply>,
    sent: i64,
}

impl FoguObject for GrpcCall {
    fn name(&self) -> &str {
        "GrpcCall"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "method" => Some(Value::Str(self.method.clone().into())),
            "metadata" => Some(metadata_value(&self.metadata)),
            "peer" => Some(Value::Str(self.peer.clone().into())),
            "sent" => Some(Value::Int(self.sent)),
            "cancelled" => Some(Value::Bool(self.reply.as_ref().is_none_or(|r| r.is_closed()))),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // send(msg) (write: the same, for pipe)
            "send" | "write" => {
                max_args(args, 1, method)?;
                if !self.streaming {
                    return Err(format!("{}: '{}' is not a server-streaming method (return the response instead)", method, self.method));
                }
                let bytes = encode(&self.output, args.first().unwrap_or(&Value::Nil))?;
                match &self.reply {
                    Some(r) if r.send(Ok(bytes)).is_ok() => {
                        self.sent += 1;
                        Ok(Value::Nil)
                    }
                    _ => Err(io_err!("grpc: the client went away")),
                }
            }
            // end(): no more messages (the call ends when the handler returns)
            "end" => {
                max_args(args, 0, "end")?;
                self.streaming = false;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on GrpcCall (send end)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send", "end"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn is_serve(receiver: &NativeObjectRef, method: &str) -> bool {
    method == "serve" && receiver.name() == "GrpcServer"
}

/// `srv.serve()`: runs the handlers of incoming requests (each as an `fg`
/// task) until `srv.close()`.
pub fn serve(receiver: &NativeObjectRef, args: &[Value], interp: &Interpreter, env: Rc<RefCell<Environment>>) -> Result<Value, String> {
    max_args(args, 0, "serve")?;
    let inner = receiver.inner();
    let shared = {
        let mut b = inner.borrow_mut();
        let srv = b.as_any_mut().downcast_mut::<GrpcServer>().expect("GrpcServer");
        if srv.task.is_none() {
            return Err("serve: call listen(port) first".into());
        }
        if srv.serving {
            return Err("serve: the server is already serving".into());
        }
        srv.serving = true;
        Arc::clone(&srv.shared)
    };
    // requests come from other threads: waiting for them is not a deadlock
    let guard = IoGuard::new();
    let result = (|| -> Result<(), String> {
        loop {
            let mut job = None;
            sched::wait_until(|| {
                job = shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                job.is_some() || shared.closed.load(Ordering::SeqCst)
            })?;
            let Some(job) = job else { return Ok(()) };
            let handler = {
                let b = inner.borrow();
                b.as_any().downcast_ref::<GrpcServer>().and_then(|s| s.handlers.get(&job.path).cloned().map(|h| (h, s.interceptors.clone())))
            };
            let Some(((f, m), interceptors)) = handler else {
                let _ = job.reply.send(Err(Status::unimplemented(format!("unknown method {}", job.path))));
                continue;
            };
            let env = Rc::clone(&env);
            interp.spawn_task(Box::new(move |it| {
                run_handler(it, env, f, m, job, interceptors);
                Ok(())
            }))?;
        }
    })();
    drop(guard);
    if let Some(s) = inner.borrow_mut().as_any_mut().downcast_mut::<GrpcServer>() {
        s.serving = false;
    }
    result.map(|_| Value::Nil)
}

fn clean(e: &str) -> String {
    e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX).to_string()
}

fn send_one(m: &Method, reply: &Reply, item: &Value) -> Result<(), Status> {
    let b = encode(&m.output, item).map_err(|e| Status::internal(format!("bad response: {}", clean(&e))))?;
    reply.send(Ok(b)).map_err(|_| Status::cancelled("the client went away"))
}

/// A handler that returns a stream (`fogu:stream`, the requests, a readable):
/// each value is a response, and an error of the stream fails the call.
fn send_stream(it: &Interpreter, o: &NativeObjectRef, m: &Method, reply: &Reply) -> Result<(), Status> {
    while let Some(item) = it.stream_next(o).map_err(|e| status_of(&e))? {
        send_one(m, reply, &item)?;
    }
    match o.get("error") {
        Some(Value::Str(e)) => Err(status_of(&e)),
        Some(Value::Error(e)) => Err(status_of(&e.message)),
        _ => Ok(()),
    }
}

fn run_handler(it: &Interpreter, env: Rc<RefCell<Environment>>, f: Value, m: Method, job: Job, interceptors: Vec<Value>) {
    let reply = job.reply.clone();
    let fail = |s: Status| {
        let _ = reply.send(Err(s));
    };
    // the request, or a stream of them
    let reader = job.incoming.map(|s| {
        NativeObjectRef::new(GrpcStream::new(read_messages(async move { Ok(s) }), m.input.clone(), None))
    });
    let arg = match &reader {
        Some(r) => r.to_value(),
        None => match job.messages.first().map(|raw| decode(&m.input, raw)) {
            Some(Ok(v)) => v,
            Some(Err(e)) => return fail(Status::invalid_argument(clean(&e))),
            None => return fail(Status::invalid_argument("no request message")),
        },
    };
    let call = NativeObjectRef::new(GrpcCall {
        method: m.path.trim_start_matches('/').to_string(),
        metadata: job.metadata,
        peer: job.peer,
        output: m.output.clone(),
        streaming: m.server_streaming,
        reply: Some(job.reply),
        sent: 0,
    });
    let outcome = |r: Result<Value, String>| match r {
        Err(e) => Err(status_of(&e)),
        // `return reply, nil` / `return nil, error(...)`
        Ok(v) => v.into_result().map_err(|e| status_of(&e.message)),
    };
    let mut out = Ok(Value::Nil);
    for ic in interceptors {
        out = outcome(it.call_value(ic, vec![call.to_value()], Rc::clone(&env)));
        if out.is_err() {
            break;
        }
    }
    if out.is_ok() {
        out = outcome(it.call_value(f, vec![arg, call.to_value()], env));
    }
    let sent = match out {
        Err(s) => Err(s),
        Ok(v) if m.server_streaming => match v {
            // returning a list, a message or a stream also streams it
            Value::Nil => Ok(()),
            Value::List(l) => {
                let items = l.borrow().clone();
                items.iter().try_for_each(|item| send_one(&m, &reply, item))
            }
            Value::Object(o) if crate::runtime::interpreter::is_stream(&o) || fogu_core::native::vstream::is_value_stream(&o) => {
                send_stream(it, &o, &m, &reply)
            }
            v => send_one(&m, &reply, &v),
        },
        Ok(v) => send_one(&m, &reply, &v),
    };
    if let Err(s) = sent {
        fail(s);
    }
    // the call and the requests may outlive the handler (kept in a variable): close them now
    if let Some(c) = call.inner().borrow_mut().as_any_mut().downcast_mut::<GrpcCall>() {
        c.reply = None;
    }
    if let Some(r) = reader {
        let _ = r.call("close", &[]);
    }
}

// ---------------------------------------------------------------------
// client
// ---------------------------------------------------------------------

pub struct GrpcClient {
    pool: DescriptorPool,
    /// `None` after `close()`: the connection goes once its calls end.
    channel: Option<tonic::transport::Channel>,
    url: String,
    timeout: Option<Duration>,
    metadata: Vec<(String, String)>,
}

fn connect(a: &[Value]) -> Result<Value, String> {
    max_args(a, 3, "grpc.connect")?;
    let url = str_arg(a, 0, "grpc.connect")?;
    let url = if url.contains("://") { url } else { format!("http://{}", url) };
    let pool = pool_of(&a[1]).map_err(|e| format!("grpc.connect: {}", e))?;
    let o = opts_map(opt_arg(a, 2), "grpc.connect")?;
    let timeout = o.get("timeout").map(|v| duration_arg(v, "grpc.connect timeout")).transpose()?;
    let metadata = metadata_opt(&o)?;
    let mut ep = tonic::transport::Endpoint::from_shared(url.clone())
        .map_err(|e| format!("grpc.connect: invalid URL '{}': {}", url, e))?
        .connect_timeout(timeout.unwrap_or(Duration::from_secs(10)))
        .tcp_nodelay(true);
    if url.starts_with("https://") {
        let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
        if let Some(ca) = o.get("ca") {
            let pem = std::fs::read(ca.to_display_string()).map_err(|e| io_err!("grpc.connect: ca {}: {}", ca.to_display_string(), e))?;
            tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
        }
        if let Some(d) = o.get("domain") {
            tls = tls.domain_name(d.to_display_string());
        }
        // { cert, key }: a client certificate, for servers that ask for one (mTLS)
        match (o.get("cert"), o.get("key")) {
            (Some(c), Some(k)) => {
                let read = |v: &Value, what: &str| std::fs::read(v.to_display_string()).map_err(|e| io_err!("grpc.connect: {} {}: {}", what, v.to_display_string(), e));
                tls = tls.identity(tonic::transport::Identity::from_pem(read(c, "cert")?, read(k, "key")?));
            }
            (None, None) => {}
            _ => return Err("grpc.connect: a client certificate needs both 'cert' and 'key'".into()),
        }
        ep = ep.tls_config(tls).map_err(|e| io_err!("grpc.connect: TLS: {}", e))?;
    } else if o.contains_key("cert") || o.contains_key("key") {
        return Err("grpc.connect: 'cert' and 'key' need an https:// URL".into());
    }
    let channel = {
        let _g = crate::native::rt::tokio().enter();
        ep.connect_lazy()
    };
    Ok(NativeObjectRef::new(GrpcClient { pool, channel: Some(channel), url, timeout, metadata }).to_value())
}

/// One call: the request messages → a stream of response messages.
async fn start_call<S>(
    channel: tonic::transport::Channel,
    path: String,
    msgs: S,
    metadata: Vec<(String, String)>,
    timeout: Option<Duration>,
) -> Result<tonic::Streaming<Vec<u8>>, String>
where
    S: tokio_stream::Stream<Item = Vec<u8>> + Send + 'static,
{
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.map_err(|e| io_err!("grpc: UNAVAILABLE: {}", e))?;
    let mut req = tonic::Request::new(msgs);
    for (k, v) in metadata {
        // `-bin` keys carry binary values (base64 on the wire); a bad key or
        // value is an error value of the call, not the end of the program
        if k.ends_with("-bin") {
            let key = tonic::metadata::MetadataKey::<tonic::metadata::Binary>::from_bytes(k.as_bytes())
                .map_err(|_| io_err!("grpc: invalid metadata key '{}'", k))?;
            req.metadata_mut().insert_bin(key, tonic::metadata::MetadataValue::from_bytes(v.as_bytes()));
            continue;
        }
        let key = tonic::metadata::MetadataKey::<tonic::metadata::Ascii>::from_bytes(k.as_bytes())
            .map_err(|_| io_err!("grpc: invalid metadata key '{}'", k))?;
        let val = v.parse().map_err(|_| io_err!("grpc: invalid metadata value for '{}'", k))?;
        req.metadata_mut().insert(key, val);
    }
    if let Some(t) = timeout {
        req.set_timeout(t);
    }
    let path = http::uri::PathAndQuery::try_from(path.as_str()).map_err(|e| format!("grpc: {}", e))?;
    let resp = grpc.streaming(req, path, RawCodec).await.map_err(|s| status_err(&s))?;
    Ok(resp.into_inner())
}

/// The client enforces the deadline too (the server only gets `grpc-timeout`).
async fn with_deadline<T>(timeout: Option<Duration>, fut: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    match timeout {
        None => fut.await,
        Some(t) => tokio::time::timeout(t, fut).await.unwrap_or_else(|_| Err(io_err!("grpc: DEADLINE_EXCEEDED: no answer within {:?}", t))),
    }
}

/// The encoded request(s) of a call; `None`: a client-streaming call opened
/// without them, whose requests go one at a time with `send`.
type Requests = Option<Vec<Vec<u8>>>;

impl GrpcClient {
    /// The method, the encoded request(s) and the call options.
    fn prepare(&self, args: &[Value], what: &str) -> Result<(Method, Requests, Vec<(String, String)>, Option<Duration>), String> {
        max_args(args, 3, what)?;
        let m = find_method(&self.pool, &str_arg(args, 0, what)?)?;
        let msgs = match args.get(1) {
            Some(Value::List(l)) if m.client_streaming => Some(l.borrow().iter().map(|v| encode(&m.input, v)).collect::<Result<_, _>>()?),
            None | Some(Value::Nil) if m.client_streaming && what == "stream" => None,
            Some(other) if m.client_streaming => {
                return Err(format!(
                    "{}: '{}' is client-streaming: pass a list of requests{}, got '{}'",
                    what,
                    m.path,
                    if what == "stream" { " (or nil, to send them one at a time)" } else { "" },
                    other.type_name()
                ))
            }
            None if m.client_streaming => return Err(format!("call: '{}' is client-streaming: pass a list of requests, or use stream() to send them one at a time", m.path)),
            req => Some(vec![encode(&m.input, req.unwrap_or(&Value::Map(Default::default())))?]),
        };
        let o = opts_map(args.get(2), what)?;
        let mut md = self.metadata.clone();
        md.extend(metadata_opt(&o)?);
        let timeout = match o.get("timeout") {
            Some(v) => Some(duration_arg(v, "timeout")?),
            None => self.timeout,
        };
        Ok((m, msgs, md, timeout))
    }

    fn channel(&self) -> Result<tonic::transport::Channel, String> {
        self.channel.clone().ok_or_else(|| io_err!("grpc: the client is closed"))
    }
}

impl FoguObject for GrpcClient {
    fn name(&self) -> &str {
        "GrpcClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "url" => Some(Value::Str(self.url.clone().into())),
            "closed" => Some(Value::Bool(self.channel.is_none())),
            "methods" => Some(list(
                self.pool.services().flat_map(|s| s.methods().map(|m| Value::Str(format!("{}/{}", s.full_name(), m.name()).into())).collect::<Vec<_>>()).collect(),
            )),
            _ => None,
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        match method {
            // call(method, request, { timeout, metadata }) → response (a list for server streaming)
            "call" => Some((|| {
                let (m, msgs, md, timeout) = self.prepare(args, "call")?;
                let msgs = msgs.unwrap_or_default();
                let channel = self.channel()?;
                let path = m.path.clone();
                let pending = crate::native::rt::spawn(async move {
                    let call = async {
                        let mut s = start_call(channel, path, tokio_stream::iter(msgs), md, timeout).await?;
                        let mut out = Vec::new();
                        while let Some(msg) = s.message().await.map_err(|s| status_err(&s))? {
                            out.push(msg);
                        }
                        Ok::<_, String>(out)
                    };
                    with_deadline(timeout, call).await
                });
                let poller: Poller = Box::new(move || {
                    let r = pending.try_take()?;
                    Some(r.and_then(|msgs| {
                        let vals = msgs.iter().map(|b| decode(&m.output, b)).collect::<Result<Vec<_>, _>>()?;
                        if m.server_streaming {
                            Ok(list(vals))
                        } else {
                            vals.into_iter().next().ok_or_else(|| io_err!("grpc: INTERNAL: the server sent no response"))
                        }
                    }))
                });
                Ok(poller)
            })()),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // stream(method, request, opts) → GrpcStream: its responses one at a time (next, for,
            // fogu:stream); a client-streaming method opened without requests sends them with send()
            "stream" => {
                let (m, msgs, md, timeout) = self.prepare(args, "stream")?;
                let channel = self.channel()?;
                let path = m.path.clone();
                let (reading, sender) = match msgs {
                    Some(msgs) => (read_messages(async move { with_deadline(timeout, start_call(channel, path, tokio_stream::iter(msgs), md, timeout)).await }), None),
                    None => {
                        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
                        let requests = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
                        (read_messages(async move { with_deadline(timeout, start_call(channel, path, requests, md, timeout)).await }), Some((tx, m.input.clone())))
                    }
                };
                Ok(NativeObjectRef::new(GrpcStream::new(reading, m.output, sender)).to_value())
            }
            // close(): no new calls; the connection goes once the open ones end
            "close" => {
                max_args(args, 0, "close")?;
                self.channel = None;
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on GrpcClient ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["call", "stream", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------
// streams of messages
// ---------------------------------------------------------------------

type StreamRx = std::sync::mpsc::Receiver<Result<Vec<u8>, String>>;

/// Reads the messages of `start`'s stream on the tokio runtime, for `next()`.
fn read_messages<F>(start: F) -> (StreamRx, tokio::task::JoinHandle<()>)
where
    F: std::future::Future<Output = Result<tonic::Streaming<Vec<u8>>, String>> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<u8>, String>>();
    // the reader waits for the network: not a deadlock
    let guard = IoGuard::new();
    let task = crate::native::rt::tokio().spawn(async move {
        match start.await {
            Err(e) => {
                let _ = tx.send(Err(e));
            }
            Ok(mut s) => loop {
                match s.message().await {
                    Ok(Some(b)) => {
                        if tx.send(Ok(b)).is_err() {
                            break;
                        }
                        sched::wake();
                    }
                    Ok(None) => break,
                    Err(st) => {
                        let _ = tx.send(Err(status_err(&st)));
                        break;
                    }
                }
            },
        }
        // the end must be visible before the scheduler wakes: the sender goes
        // first, then the guard (whose drop wakes it to see `Disconnected`)
        drop(tx);
        drop(guard);
    });
    (rx, task)
}

#[derive(Default)]
struct StreamState {
    rx: Option<StreamRx>,
    received: i64,
    /// The error that ended the stream (`s.error`, and what `fogu:stream` fails with).
    error: Option<String>,
}

/// The messages of a streaming call, one at a time: the responses on the
/// client (`c.stream`), the requests in a client-streaming handler. It is a
/// stream (`for m in s`, `stream.from(s)`), and a client stream opened
/// without requests also sends them (`send`, `end`, or `pipe` into it).
pub struct GrpcStream {
    state: Rc<RefCell<StreamState>>,
    /// What `next()` decodes.
    output: MessageDescriptor,
    task: Option<tokio::task::JoinHandle<()>>,
    /// The requests still to send, and their type.
    sender: Option<(tokio::sync::mpsc::UnboundedSender<Vec<u8>>, MessageDescriptor)>,
    sends: bool,
    sent: i64,
}

impl GrpcStream {
    fn new(
        (rx, task): (StreamRx, tokio::task::JoinHandle<()>),
        output: MessageDescriptor,
        sender: Option<(tokio::sync::mpsc::UnboundedSender<Vec<u8>>, MessageDescriptor)>,
    ) -> GrpcStream {
        let state = StreamState { rx: Some(rx), ..Default::default() };
        GrpcStream { state: Rc::new(RefCell::new(state)), output, task: Some(task), sends: sender.is_some(), sender, sent: 0 }
    }
}

/// One step of reading: `None` while nothing arrived.
fn poll_message(state: &RefCell<StreamState>, out: &MessageDescriptor) -> Option<Result<Option<Value>, String>> {
    let mut st = state.borrow_mut();
    let r = st.rx.as_ref()?.try_recv();
    match r {
        Ok(Ok(b)) => {
            st.received += 1;
            Some(decode(out, &b).map(Some))
        }
        Ok(Err(e)) => {
            st.rx = None;
            st.error = Some(clean(&e));
            Some(Err(e))
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            st.rx = None;
            Some(Ok(None))
        }
    }
}

impl FoguObject for GrpcStream {
    fn name(&self) -> &str {
        "GrpcStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let st = self.state.borrow();
        match field {
            "done" => Some(Value::Bool(st.rx.is_none())),
            "received" => Some(Value::Int(st.received)),
            "sent" => Some(Value::Int(self.sent)),
            "error" => Some(st.error.as_ref().map_or(Value::Nil, |e| Value::Str(e.clone().into()))),
            _ => None,
        }
    }

    // next() → the next message, nil at the end, or the call's error · toList() → the rest
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if !matches!(method, "next" | "toList") {
            return None;
        }
        if let Err(e) = max_args(args, 0, method) {
            return Some(Err(e));
        }
        let state = Rc::clone(&self.state);
        let out = self.output.clone();
        if method == "next" {
            return Some(Ok(Box::new(move || {
                if state.borrow().rx.is_none() {
                    return Some(Ok(Value::Nil));
                }
                poll_message(&state, &out).map(|r| r.map(|v| v.unwrap_or(Value::Nil)))
            })));
        }
        let mut items = Vec::new();
        Some(Ok(Box::new(move || loop {
            if state.borrow().rx.is_none() {
                return Some(Ok(list(std::mem::take(&mut items))));
            }
            match poll_message(&state, &out)? {
                Ok(Some(v)) => items.push(v),
                Ok(None) => return Some(Ok(list(std::mem::take(&mut items)))),
                Err(e) => return Some(Err(e)),
            }
        })))
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // send(msg) (write: the same, for pipe): one more request
            "send" | "write" => {
                max_args(args, 1, method)?;
                let Some((tx, input)) = &self.sender else {
                    return Err(if self.sends {
                        format!("{}: the requests of this stream have ended", method)
                    } else {
                        format!("{}: this stream does not send (open a client-streaming method with c.stream(method) to send requests)", method)
                    });
                };
                let bytes = encode(input, args.first().unwrap_or(&Value::Nil))?;
                tx.send(bytes).map_err(|_| io_err!("grpc: the call has ended"))?;
                self.sent += 1;
                Ok(Value::Nil)
            }
            // end(): no more requests; the responses go on
            "end" => {
                max_args(args, 0, "end")?;
                self.sender = None;
                Ok(Value::Nil)
            }
            // close(): stops the call
            "close" => {
                self.sender = None;
                self.state.borrow_mut().rx = None;
                if let Some(t) = self.task.take() {
                    t.abort();
                }
                Ok(Value::Nil)
            }
            // a `for` loop or a fogu:stream that met the error records it
            "__fail" => {
                if let Some(v) = args.first() {
                    let msg = match v {
                        Value::Error(e) => e.message.to_string(),
                        v => v.to_display_string(),
                    };
                    self.state.borrow_mut().error.get_or_insert(clean(&msg));
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on GrpcStream (next send end close toList)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "send", "end", "close", "toList"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Drop for GrpcStream {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        assert_eq!(status_of("NOT_FOUND: no user").code(), Code::NotFound);
        assert_eq!(status_of("NOT_FOUND: no user").message(), "no user");
        assert_eq!(status_of("boom").code(), Code::Unknown);
        assert_eq!(code_name(Code::Unauthenticated), "UNAUTHENTICATED");
    }

    #[test]
    fn builtin_protocols_compile() {
        assert!(builtin_pool().get_service_by_name(HEALTH_SERVICE).is_some());
        assert!(builtin_pool().get_service_by_name(REFLECTION_SERVICE).is_some());
    }
}
