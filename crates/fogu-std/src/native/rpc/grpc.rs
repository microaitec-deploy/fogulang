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
//! srv.listen(50051)?
//! fg srv.serve()                                            // until srv.close()
//!
//! // client
//! const c = grpc.connect("http://localhost:50051", schema)?
//! print(c.call("SayHello", { name: "Ana" })?)               // { message: Hello, Ana }
//! print(c.call("Count", { n: 3 })?)                         // a list for server streaming
//! const s = c.stream("Count", { n: 3 })?                    // or one message at a time
//! for (let m = s.next()?; m != nil; m = s.next()?) { print(m) }
//! ```
//!
//! - Methods are named `"pkg.Service/Method"`, `"pkg.Service.Method"` or
//!   just `"Method"` when it is unique in the schema.
//! - Client-streaming methods take a list of requests (the handler receives
//!   the list); server-streaming ones answer with `call.send(msg)` (or by
//!   returning a list). Bidirectional methods combine both; the requests are
//!   all received before the handler runs.
//! - A handler fails the call by returning an error: `return nil,
//!   grpc.status("NOT_FOUND", "no such user")` (or `error("NOT_FOUND: ...")`);
//!   any other error is `UNKNOWN`. On the client a failed call is an error
//!   value `"grpc: NOT_FOUND: no such user"`.
//! - `https://` URLs use TLS (system roots, or `{ ca: "ca.pem" }`);
//!   `listen(port, { cert, key })` serves TLS. Metadata travels in
//!   `{ metadata: { key: value } }` and arrives as `call.metadata`.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use ahash::AHashMap as HashMap;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Buf, BufMut};
use prost_reflect::{DescriptorPool, MessageDescriptor};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::{Code, Status};

use crate::runtime::environment::Environment;
use crate::runtime::interpreter::Interpreter;
use crate::runtime::sched::{self, IoGuard};
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
    // server(schema) → GrpcServer
    e.insert("server".into(), nfn_var("server", 1, |a| {
        max_args(a, 1, "grpc.server")?;
        let pool = pool_of(&a[0]).map_err(|e| format!("grpc.server: {}", e))?;
        Ok(NativeObjectRef::new(GrpcServer::new(pool)).to_value())
    }));
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
    let m: HashMap<String, Value> = pairs.iter().map(|(k, v)| (k.clone(), Value::Str(v.clone().into()))).collect();
    Value::Map(Rc::new(RefCell::new(m.into_iter().collect())))
}

/// `{ metadata: { k: v } }` → pairs (keys lower case, as HTTP/2 wants them).
fn metadata_opt(o: &HashMap<String, Value>) -> Result<Vec<(String, String)>, String> {
    match o.get("metadata") {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_display_string())).collect()),
        Some(other) => Err(format!("grpc: 'metadata' must be a map, got '{}'", other.type_name())),
    }
}

fn opts_map(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
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
    messages: Vec<Vec<u8>>,
    metadata: Vec<(String, String)>,
    peer: String,
    reply: Reply,
}

#[derive(Default)]
struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    /// Paths with a handler (others answer UNIMPLEMENTED right away).
    known: Mutex<HashSet<String>>,
    closed: AtomicBool,
}

pub struct GrpcServer {
    pool: DescriptorPool,
    handlers: HashMap<String, (Value, Method)>,
    shared: Arc<Shared>,
    task: Option<tokio::task::JoinHandle<()>>,
    port: Option<u16>,
    host: String,
    tls: bool,
    serving: bool,
}

impl GrpcServer {
    fn new(pool: DescriptorPool) -> GrpcServer {
        GrpcServer {
            pool,
            handlers: HashMap::new(),
            shared: Arc::new(Shared::default()),
            task: None,
            port: None,
            host: String::new(),
            tls: false,
            serving: false,
        }
    }

    fn add(&mut self, name: &str, f: Value) -> Result<(), String> {
        if !matches!(f, Value::Fn(_) | Value::NativeFn(_) | Value::BoundMethod(_) | Value::StructBoundMethod(_)) {
            return Err(format!("handle: the handler of '{}' must be a function, got '{}'", name, f.type_name()));
        }
        let m = find_method(&self.pool, name)?;
        self.shared.known.lock().unwrap_or_else(|e| e.into_inner()).insert(m.path.clone());
        self.handlers.insert(m.path.clone(), (f, m));
        Ok(())
    }

    fn listen(&mut self, args: &[Value]) -> Result<Value, String> {
        max_args(args, 2, "listen")?;
        if self.task.is_some() {
            return Err("listen: the server is already listening".into());
        }
        let addr = match args.first() {
            Some(Value::Int(p)) => format!("0.0.0.0:{}", p),
            Some(Value::Str(s)) if s.starts_with(':') => format!("0.0.0.0{}", s),
            Some(Value::Str(s)) => s.to_string(),
            _ => return Err("listen(port | \"host:port\", { cert, key })".into()),
        };
        let o = opts_map(args.get(1), "listen")?;
        let tls = match (o.get("cert"), o.get("key")) {
            (Some(c), Some(k)) => Some(tls_acceptor(&c.to_display_string(), &k.to_display_string())?),
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

fn tls_acceptor(cert: &str, key: &str) -> Result<tokio_rustls::TlsAcceptor, String> {
    let certs: Vec<_> = {
        let data = std::fs::read(cert).map_err(|e| io_err!("grpc: cert {}: {}", cert, e))?;
        <rustls::pki_types::CertificateDer as rustls::pki_types::pem::PemObject>::pem_slice_iter(&data).collect::<Result<_, _>>().map_err(|e| io_err!("grpc: cert {}: {}", cert, e))?
    };
    let key = {
        let data = std::fs::read(key).map_err(|e| io_err!("grpc: key {}: {}", key, e))?;
        <rustls::pki_types::PrivateKeyDer as rustls::pki_types::pem::PemObject>::from_pem_slice(&data).map_err(|e| match e {
            rustls::pki_types::pem::Error::NoItemsFound => io_err!("grpc: no private key in {}", key),
            e => io_err!("grpc: key {}: {}", key, e),
        })?
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("grpc: TLS: {}", e))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io_err!("grpc: TLS: {}", e))?;
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
    if !shared.known.lock().unwrap_or_else(|e| e.into_inner()).contains(&path) {
        return Status::unimplemented(format!("unknown method {}", path)).into_http();
    }
    // every kind of RPC is a stream of messages each way on the wire
    tonic::server::Grpc::new(RawCodec).streaming(Handler { shared, path, peer }, req).await
}

#[derive(Clone)]
struct Handler {
    shared: Arc<Shared>,
    path: String,
    peer: String,
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
            let mut messages = Vec::new();
            while let Some(m) = stream.message().await? {
                messages.push(m);
            }
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            me.shared.jobs.lock().unwrap_or_else(|e| e.into_inner()).push_back(Job {
                path: me.path,
                messages,
                metadata,
                peer: me.peer,
                reply: tx,
            });
            sched::wake();
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
        vec!["handle", "service", "listen", "serve", "close"]
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
            "send" => {
                max_args(args, 1, "send")?;
                if !self.streaming {
                    return Err(format!("send: '{}' is not a server-streaming method (return the response instead)", self.method));
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
            other => Err(format!("method '{}' does not exist on GrpcCall (send)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["send"]
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
                b.as_any().downcast_ref::<GrpcServer>().and_then(|s| s.handlers.get(&job.path).cloned())
            };
            let Some((f, m)) = handler else {
                let _ = job.reply.send(Err(Status::unimplemented(format!("unknown method {}", job.path))));
                continue;
            };
            let env = Rc::clone(&env);
            interp.spawn_task(Box::new(move |it| {
                run_handler(it, env, f, m, job);
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

fn run_handler(it: &Interpreter, env: Rc<RefCell<Environment>>, f: Value, m: Method, job: Job) {
    let reply = job.reply.clone();
    let fail = |s: Status| {
        let _ = reply.send(Err(s));
    };
    // the request(s)
    let mut reqs = Vec::new();
    for raw in &job.messages {
        match decode(&m.input, raw) {
            Ok(v) => reqs.push(v),
            Err(e) => return fail(Status::invalid_argument(e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX).to_string())),
        }
    }
    let arg = if m.client_streaming {
        list(reqs)
    } else {
        match reqs.into_iter().next() {
            Some(v) => v,
            None => return fail(Status::invalid_argument("no request message")),
        }
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
    let r = it.call_value(f, vec![arg, call.to_value()], env);
    // the call object may outlive the handler (kept in a variable): close the stream now
    let close = || {
        if let Some(c) = call.inner().borrow_mut().as_any_mut().downcast_mut::<GrpcCall>() {
            c.reply = None;
        }
    };
    let out = match r {
        Err(e) => Err(status_of(&e)),
        Ok(Value::Error(e)) => Err(status_of(&e.message)),
        Ok(Value::List(l)) if l.borrow().len() == 2 && matches!(l.borrow()[1], Value::Error(_)) => {
            let Value::Error(e) = &l.borrow()[1] else { unreachable!() };
            Err(status_of(&e.message))
        }
        Ok(v) => Ok(v),
    };
    match out {
        Err(s) => fail(s),
        Ok(v) if m.server_streaming => {
            // returning a list (or a message) also streams it
            let items = match v {
                Value::Nil => Vec::new(),
                Value::List(l) => l.borrow().clone(),
                v => vec![v],
            };
            for item in items {
                match encode(&m.output, &item) {
                    Ok(b) => {
                        let _ = reply.send(Ok(b));
                    }
                    Err(e) => return { close(); fail(Status::internal(e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX).to_string())) },
                }
            }
        }
        Ok(v) => match encode(&m.output, &v) {
            Ok(b) => {
                let _ = reply.send(Ok(b));
            }
            Err(e) => fail(Status::internal(format!("bad response: {}", e.trim_start_matches(crate::runtime::value::ERR_VALUE_PREFIX)))),
        },
    }
    close();
}

// ---------------------------------------------------------------------
// client
// ---------------------------------------------------------------------

pub struct GrpcClient {
    pool: DescriptorPool,
    channel: tonic::transport::Channel,
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
        ep = ep.tls_config(tls).map_err(|e| io_err!("grpc.connect: TLS: {}", e))?;
    }
    let channel = {
        let _g = crate::native::rt::tokio().enter();
        ep.connect_lazy()
    };
    Ok(NativeObjectRef::new(GrpcClient { pool, channel, url, timeout, metadata }).to_value())
}

/// One call: the request messages → a stream of response messages.
async fn start_call(
    channel: tonic::transport::Channel,
    path: String,
    msgs: Vec<Vec<u8>>,
    metadata: Vec<(String, String)>,
    timeout: Option<Duration>,
) -> Result<tonic::Streaming<Vec<u8>>, String> {
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.map_err(|e| io_err!("grpc: UNAVAILABLE: {}", e))?;
    let mut req = tonic::Request::new(tokio_stream::iter(msgs));
    for (k, v) in metadata {
        let key = tonic::metadata::MetadataKey::<tonic::metadata::Ascii>::from_bytes(k.as_bytes())
            .map_err(|_| format!("grpc: invalid metadata key '{}'", k))?;
        let val = v.parse().map_err(|_| format!("grpc: invalid metadata value for '{}'", k))?;
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

impl GrpcClient {
    /// The method, the encoded request(s) and the call options.
    fn prepare(&self, args: &[Value], what: &str) -> Result<(Method, Vec<Vec<u8>>, Vec<(String, String)>, Option<Duration>), String> {
        max_args(args, 3, what)?;
        let m = find_method(&self.pool, &str_arg(args, 0, what)?)?;
        let req = args.get(1).cloned().unwrap_or(Value::Map(Default::default()));
        let msgs = if m.client_streaming {
            match &req {
                Value::List(l) => l.borrow().iter().map(|v| encode(&m.input, v)).collect::<Result<_, _>>()?,
                other => return Err(format!("{}: '{}' is client-streaming: pass a list of requests, got '{}'", what, m.path, other.type_name())),
            }
        } else {
            vec![encode(&m.input, &req)?]
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
}

impl FoguObject for GrpcClient {
    fn name(&self) -> &str {
        "GrpcClient"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "url" => Some(Value::Str(self.url.clone().into())),
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
                let channel = self.channel.clone();
                let path = m.path.clone();
                let pending = crate::native::rt::spawn(async move {
                    let call = async {
                        let mut s = start_call(channel, path, msgs, md, timeout).await?;
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
            // stream(method, request, opts) → GrpcStream (next() one message at a time)
            "stream" => {
                let (m, msgs, md, timeout) = self.prepare(args, "stream")?;
                let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<u8>, String>>();
                let channel = self.channel.clone();
                let path = m.path.clone();
                let guard = IoGuard::new();
                let task = crate::native::rt::tokio().spawn(async move {
                    let _guard = guard;
                    match with_deadline(timeout, start_call(channel, path, msgs, md, timeout)).await {
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
                });
                Ok(NativeObjectRef::new(GrpcStream { rx: Rc::new(RefCell::new(Some(rx))), output: m.output, task: Some(task), received: Rc::new(std::cell::Cell::new(0)) }).to_value())
            }
            "close" => Ok(Value::Nil),
            other => Err(format!("method '{}' does not exist on GrpcClient ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["call", "stream", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

type StreamRx = Rc<RefCell<Option<std::sync::mpsc::Receiver<Result<Vec<u8>, String>>>>>;

/// The responses of a streaming call, read one at a time with `next()`.
pub struct GrpcStream {
    rx: StreamRx,
    output: MessageDescriptor,
    task: Option<tokio::task::JoinHandle<()>>,
    received: Rc<std::cell::Cell<i64>>,
}

impl FoguObject for GrpcStream {
    fn name(&self) -> &str {
        "GrpcStream"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "done" => Some(Value::Bool(self.rx.borrow().is_none())),
            "received" => Some(Value::Int(self.received.get())),
            _ => None,
        }
    }

    // next() → the next message, nil at the end, or the call's error
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        if let Err(e) = max_args(args, 0, "next") {
            return Some(Err(e));
        }
        let received = Rc::clone(&self.received);
        let rx = Rc::clone(&self.rx);
        let out = self.output.clone();
        Some(Ok(Box::new(move || {
            let mut slot = rx.borrow_mut();
            let Some(r) = slot.as_ref() else { return Some(Ok(Value::Nil)) };
            match r.try_recv() {
                Ok(Ok(b)) => {
                    received.set(received.get() + 1);
                    Some(decode(&out, &b))
                }
                Ok(Err(e)) => {
                    *slot = None;
                    Some(Err(e))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    *slot = None;
                    Some(Ok(Value::Nil))
                }
            }
        })))
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => {
                self.rx.borrow_mut().take();
                if let Some(t) = self.task.take() {
                    t.abort();
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on GrpcStream (next close)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
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
}
