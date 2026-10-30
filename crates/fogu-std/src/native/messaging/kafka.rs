//! `fogu:kafka` module — Apache Kafka (and Redpanda) client in pure Rust
//! (rskafka): topics, producing and consuming, TLS and SASL.
//!
//! ```fogu
//! import kafka from "fogu:kafka"
//!
//! const k = kafka.connect(["localhost:9092"])?               // { tls, sasl: { mechanism, username, password } }
//! k.createTopic("orders", { partitions: 3 })?
//! print(k.send("orders", { id: 1, total: 9.5 }, { key: "client-7" })?)   // { partition, offset }
//! const c = k.consumer("orders", { from: "earliest" })?
//! const r = c.next(2s)?                                       // nil after 2s without records
//! print(r.partition, r.offset, r.key, r.json())
//! ```
//!
//! Values: a string is sent as UTF-8, other values as JSON; records arrive
//! with `value` as text (`r.json()` parses it). Without a `partition`, a
//! record with a key goes to `hash(key) % partitions` (the same key keeps
//! its order), one without a key round-robins.
//!
//! Consumers read every partition of the topic (or the `partitions` given)
//! and keep their own offsets (`c.offsets`, `c.seek`): Kafka consumer
//! groups (coordinated rebalancing, offsets stored in Kafka) are not
//! supported.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use ahash::AHashMap as HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rskafka::client::partition::{Compression, OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder, Credentials, SaslConfig};
use rskafka::record::{Record, RecordAndOffset};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{bytes_value, duration_arg, list, map, max_args, nfn_var, opt_arg, str_arg};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    ModuleValue {
        name: "fogu:kafka".into(),
        exports: RefCell::new(e),
    }
}

fn kerr(what: &str, e: impl std::fmt::Display) -> String {
    io_err!("kafka.{}: {}", what, e)
}

fn opts(v: Option<&Value>, what: &str) -> Result<HashMap<String, Value>, String> {
    match v {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn int_opt(o: &HashMap<String, Value>, k: &str) -> Option<i64> {
    match o.get(k) {
        Some(Value::Int(i)) => Some(*i),
        _ => None,
    }
}

/// A future on the shared runtime, waited for cooperatively by the interpreter.
fn poll_future<T: Send + 'static>(
    fut: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    finish: impl FnOnce(T) -> Result<Value, String> + 'static,
) -> Poller {
    let pending = crate::native::rt::spawn(fut);
    let mut finish = Some(finish);
    Box::new(move || {
        let r = pending.try_take()?;
        Some(r.and_then(|v| (finish.take().expect("polled once"))(v)))
    })
}

/// connect(brokers, { clientId, tls, sasl, timeout = 10s }) → Kafka
fn connect(a: &[Value]) -> Result<Value, String> {
    max_args(a, 2, "kafka.connect")?;
    let brokers: Vec<String> = match &a[0] {
        Value::Str(s) => s.split(',').map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).collect(),
        Value::List(l) => l.borrow().iter().map(|v| v.to_display_string()).collect(),
        other => return Err(format!("kafka.connect: brokers must be a string or a list, got '{}'", other.type_name())),
    };
    let o = opts(a.get(1), "kafka.connect")?;
    let mut b = ClientBuilder::new(brokers.clone());
    if let Some(id) = o.get("clientId") {
        b = b.client_id(id.to_display_string());
    }
    match o.get("tls") {
        None | Some(Value::Nil) | Some(Value::Bool(false)) => {}
        Some(t) => {
            let ca = match t {
                Value::Map(m) => m.borrow().get("ca").map(|v| v.to_display_string()),
                _ => None,
            };
            b = b.tls_config(Arc::new(tls_config(ca.as_deref())?));
        }
    }
    if let Some(Value::Map(s)) = o.get("sasl") {
        let s = s.borrow();
        let get = |k: &str| s.get(k).map(|v| v.to_display_string()).unwrap_or_default();
        let cred = Credentials::new(get("username"), get("password"));
        b = b.sasl_config(match get("mechanism").to_ascii_uppercase().as_str() {
            "" | "PLAIN" => SaslConfig::Plain(cred),
            "SCRAM-SHA-256" => SaslConfig::ScramSha256(cred),
            "SCRAM-SHA-512" => SaslConfig::ScramSha512(cred),
            other => return Err(format!("kafka.connect: unknown SASL mechanism '{}' (PLAIN SCRAM-SHA-256 SCRAM-SHA-512)", other)),
        });
    }
    let timeout = match o.get("timeout") {
        Some(t) => duration_arg(t, "kafka.connect timeout")?,
        None => Duration::from_secs(10),
    };
    let list = brokers.join(",");
    let client = crate::native::rt::block_on(async move { tokio::time::timeout(timeout, b.build()).await })?
        .map_err(|_| io_err!("kafka.connect: {}: timed out after {:?}", list, timeout))?
        .map_err(|e| kerr("connect", e))?;
    Ok(NativeObjectRef::new(Kafka { inner: Arc::new(Inner { client, parts: Mutex::default(), rr: Mutex::default() }), brokers }).to_value())
}

fn tls_config(ca: Option<&str>) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    match ca {
        Some(path) => {
            let data = std::fs::read(path).map_err(|e| io_err!("kafka.connect: ca {}: {}", path, e))?;
            for c in <rustls::pki_types::CertificateDer as rustls::pki_types::pem::PemObject>::pem_slice_iter(&data) {
                let c = c.map_err(|e| io_err!("kafka.connect: ca {}: {}", path, e))?;
                roots.add(c).map_err(|e| io_err!("kafka.connect: ca {}: {}", path, e))?;
            }
        }
        None => {
            for c in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(c);
            }
        }
    }
    Ok(rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| io_err!("kafka.connect: TLS: {}", e))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

struct Inner {
    client: Client,
    /// partition clients, by topic and partition
    parts: Mutex<HashMap<(String, i32), Arc<PartitionClient>>>,
    /// round-robin counters for records without a key
    rr: Mutex<HashMap<String, usize>>,
}

impl Inner {
    async fn partitions(&self, topic: &str) -> Result<Vec<i32>, String> {
        let topics = self.client.list_topics().await.map_err(|e| kerr("topics", e))?;
        topics
            .into_iter()
            .find(|t| t.name == topic)
            .map(|t| t.partitions.into_iter().collect())
            .ok_or_else(|| io_err!("kafka: topic '{}' does not exist", topic))
    }

    async fn partition(&self, topic: &str, p: i32) -> Result<Arc<PartitionClient>, String> {
        let key = (topic.to_string(), p);
        if let Some(c) = self.parts.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(Arc::clone(c));
        }
        let c = Arc::new(self.client.partition_client(topic, p, UnknownTopicHandling::Retry).await.map_err(|e| kerr("partition", e))?);
        self.parts.lock().unwrap_or_else(|e| e.into_inner()).insert(key, Arc::clone(&c));
        Ok(c)
    }
}

/// A record to send, as plain data.
struct Out {
    key: Option<Vec<u8>>,
    value: Option<Vec<u8>>,
    headers: BTreeMap<String, Vec<u8>>,
}

fn payload(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Nil => None,
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        v => Some(crate::native::json::stringify_value(v).into_bytes()),
    }
}

/// `value` or `{ value, key, headers }` (for sendBatch) + options → a record.
fn out_record(value: &Value, o: &HashMap<String, Value>) -> Out {
    let headers = match o.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string().into_bytes())).collect(),
        _ => BTreeMap::new(),
    };
    Out { key: o.get("key").and_then(payload), value: payload(value), headers }
}

/// FNV-1a: the same key always lands on the same partition.
fn key_hash(k: &[u8]) -> u64 {
    k.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

fn compression(o: &HashMap<String, Value>) -> Result<Compression, String> {
    Ok(match o.get("compression").map(|v| v.to_display_string()).as_deref() {
        None | Some("none") => Compression::NoCompression,
        Some("gzip") => Compression::Gzip,
        Some("lz4") => Compression::Lz4,
        Some("snappy") => Compression::Snappy,
        Some("zstd") => Compression::Zstd,
        Some(other) => return Err(format!("kafka: unknown compression '{}' (none gzip lz4 snappy zstd)", other)),
    })
}

pub struct Kafka {
    inner: Arc<Inner>,
    brokers: Vec<String>,
}

impl FoguObject for Kafka {
    fn name(&self) -> &str {
        "Kafka"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "brokers").then(|| list(self.brokers.iter().map(|b| Value::Str(b.clone().into())).collect()))
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let inner = Arc::clone(&self.inner);
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // topics() → [{ name, partitions }]
                "topics" => {
                    max_args(args, 0, "topics")?;
                    poll_future(
                        async move { inner.client.list_topics().await.map_err(|e| kerr("topics", e)) },
                        |ts| {
                            let mut ts: Vec<_> = ts.into_iter().filter(|t| !t.name.starts_with("__")).collect();
                            ts.sort_by(|a, b| a.name.cmp(&b.name));
                            Ok(list(
                                ts.into_iter()
                                    .map(|t| map(vec![("name", Value::Str(t.name.into())), ("partitions", Value::Int(t.partitions.len() as i64))]))
                                    .collect(),
                            ))
                        },
                    )
                }
                // createTopic(name, { partitions = 1, replication = 1 }) → true (false if it existed)
                "createTopic" => {
                    max_args(args, 2, "createTopic")?;
                    let name = str_arg(args, 0, "createTopic")?;
                    let o = opts(args.get(1), "createTopic")?;
                    let parts = int_opt(&o, "partitions").unwrap_or(1) as i32;
                    let repl = int_opt(&o, "replication").unwrap_or(1) as i16;
                    poll_future(
                        async move {
                            let c = inner.client.controller_client().map_err(|e| kerr("createTopic", e))?;
                            match c.create_topic(name, parts, repl, 10_000).await {
                                Ok(()) => Ok(true),
                                Err(e) if e.to_string().contains("TopicAlreadyExists") => Ok(false),
                                Err(e) => Err(kerr("createTopic", e)),
                            }
                        },
                        |created| Ok(Value::Bool(created)),
                    )
                }
                "deleteTopic" => {
                    max_args(args, 1, "deleteTopic")?;
                    let name = str_arg(args, 0, "deleteTopic")?;
                    poll_future(
                        async move {
                            let c = inner.client.controller_client().map_err(|e| kerr("deleteTopic", e))?;
                            c.delete_topic(name, 10_000).await.map_err(|e| kerr("deleteTopic", e))
                        },
                        |_| Ok(Value::Nil),
                    )
                }
                // offsets(topic) → { partition: { earliest, latest } }
                "offsets" => {
                    max_args(args, 1, "offsets")?;
                    let topic = str_arg(args, 0, "offsets")?;
                    poll_future(
                        async move {
                            let mut out = Vec::new();
                            for p in inner.partitions(&topic).await? {
                                let c = inner.partition(&topic, p).await?;
                                let e = c.get_offset(OffsetAt::Earliest).await.map_err(|e| kerr("offsets", e))?;
                                let l = c.get_offset(OffsetAt::Latest).await.map_err(|e| kerr("offsets", e))?;
                                out.push((p, e, l));
                            }
                            Ok(out)
                        },
                        |v| {
                            let m: HashMap<String, Value> = v
                                .into_iter()
                                .map(|(p, e, l)| (p.to_string(), map(vec![("earliest", Value::Int(e)), ("latest", Value::Int(l))])))
                                .collect();
                            Ok(Value::Map(Rc::new(RefCell::new(m.into_iter().collect()))))
                        },
                    )
                }
                // send(topic, value, { key, headers, partition, compression }) → { partition, offset }
                "send" => {
                    max_args(args, 3, "send")?;
                    let topic = str_arg(args, 0, "send")?;
                    let o = opts(args.get(2), "send")?;
                    let rec = out_record(args.get(1).unwrap_or(&Value::Nil), &o);
                    let part = int_opt(&o, "partition").map(|p| p as i32);
                    let comp = compression(&o)?;
                    poll_future(
                        async move {
                            let offs = produce(&inner, &topic, part, vec![rec], comp).await?;
                            Ok(offs.into_iter().next().expect("one record"))
                        },
                        |(p, off)| Ok(map(vec![("partition", Value::Int(p as i64)), ("offset", Value::Int(off))])),
                    )
                }
                // sendBatch(topic, [value | { value, key, headers }], { partition, compression }) → [{ partition, offset }]
                "sendBatch" => {
                    max_args(args, 3, "sendBatch")?;
                    let topic = str_arg(args, 0, "sendBatch")?;
                    let o = opts(args.get(2), "sendBatch")?;
                    let items = match args.get(1) {
                        Some(Value::List(l)) => l.borrow().clone(),
                        _ => return Err("sendBatch(topic, [records], options?)".into()),
                    };
                    let recs = items
                        .iter()
                        .map(|v| match v {
                            Value::Map(m) if m.borrow().contains_key("value") => {
                                let ro = m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                                out_record(m.borrow().get("value").unwrap_or(&Value::Nil), &ro)
                            }
                            v => out_record(v, &HashMap::new()),
                        })
                        .collect();
                    let part = int_opt(&o, "partition").map(|p| p as i32);
                    let comp = compression(&o)?;
                    poll_future(async move { produce(&inner, &topic, part, recs, comp).await }, |offs| {
                        Ok(list(offs.into_iter().map(|(p, off)| map(vec![("partition", Value::Int(p as i64)), ("offset", Value::Int(off))])).collect()))
                    })
                }
                // consumer(topic, { from = "latest" | "earliest" | offset, partitions, maxWait = 500ms, maxBytes }) → Consumer
                "consumer" => {
                    max_args(args, 2, "consumer")?;
                    let topic = str_arg(args, 0, "consumer")?;
                    let o = opts(args.get(1), "consumer")?;
                    let from = match o.get("from") {
                        None | Some(Value::Nil) => From::Latest,
                        Some(Value::Int(n)) => From::Offset(*n),
                        Some(v) => match v.to_display_string().as_str() {
                            "earliest" | "beginning" => From::Earliest,
                            "latest" | "end" => From::Latest,
                            other => return Err(format!("consumer: from must be \"earliest\", \"latest\" or an offset, got '{}'", other)),
                        },
                    };
                    let only: Option<Vec<i32>> = match o.get("partitions") {
                        Some(Value::List(l)) => Some(l.borrow().iter().filter_map(|v| if let Value::Int(i) = v { Some(*i as i32) } else { None }).collect()),
                        _ => None,
                    };
                    let max_wait = match o.get("maxWait") {
                        Some(v) => duration_arg(v, "consumer maxWait")?,
                        None => Duration::from_millis(500),
                    };
                    let max_bytes = int_opt(&o, "maxBytes").unwrap_or(1 << 20) as i32;
                    let t2 = topic.clone();
                    let inner2 = Arc::clone(&inner);
                    poll_future(
                        async move {
                            let parts = match only {
                                Some(p) => p,
                                None => inner.partitions(&topic).await?,
                            };
                            let mut offsets = BTreeMap::new();
                            for p in parts {
                                let c = inner.partition(&topic, p).await?;
                                let o = match from {
                                    From::Earliest => c.get_offset(OffsetAt::Earliest).await.map_err(|e| kerr("consumer", e))?,
                                    From::Latest => c.get_offset(OffsetAt::Latest).await.map_err(|e| kerr("consumer", e))?,
                                    From::Offset(n) => n,
                                };
                                offsets.insert(p, o);
                            }
                            Ok(offsets)
                        },
                        move |offsets| {
                            Ok(NativeObjectRef::new(Consumer {
                                st: Rc::new(RefCell::new(ConsumerState { buffer: VecDeque::new(), offsets, inflight: None })),
                                inner: inner2,
                                topic: t2,
                                max_wait,
                                max_bytes,
                            })
                            .to_value())
                        },
                    )
                }
                _ => return Ok(None),
            }))
        })();
        match r {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "close" => Ok(Value::Nil),
            other => Err(format!("method '{}' does not exist on Kafka ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["topics", "createTopic", "deleteTopic", "offsets", "send", "sendBatch", "consumer", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Clone, Copy)]
enum From {
    Earliest,
    Latest,
    Offset(i64),
}

/// Produces the records (grouped by partition) → (partition, offset) in order.
async fn produce(inner: &Inner, topic: &str, part: Option<i32>, recs: Vec<Out>, comp: Compression) -> Result<Vec<(i32, i64)>, String> {
    let parts = match part {
        Some(p) => vec![p],
        None => inner.partitions(topic).await?,
    };
    let n = parts.len().max(1);
    let mut groups: BTreeMap<i32, Vec<(usize, Record)>> = BTreeMap::new();
    for (i, r) in recs.into_iter().enumerate() {
        let p = match (part, &r.key) {
            (Some(p), _) => p,
            (None, Some(k)) => parts[(key_hash(k) % n as u64) as usize],
            (None, None) => {
                let mut rr = inner.rr.lock().unwrap_or_else(|e| e.into_inner());
                let c = rr.entry(topic.to_string()).or_insert(0);
                *c = c.wrapping_add(1);
                parts[*c % n]
            }
        };
        let rec = Record { key: r.key, value: r.value, headers: r.headers, timestamp: rskafka::chrono::Utc::now() };
        groups.entry(p).or_default().push((i, rec));
    }
    let mut out = Vec::new();
    for (p, items) in groups {
        let c = inner.partition(topic, p).await?;
        let (idx, recs): (Vec<usize>, Vec<Record>) = items.into_iter().unzip();
        let offs = c.produce(recs, comp).await.map_err(|e| kerr("send", e))?;
        out.extend(idx.into_iter().zip(offs).map(|(i, o)| (i, p, o)));
    }
    out.sort_by_key(|(i, _, _)| *i);
    Ok(out.into_iter().map(|(_, p, o)| (p, o)).collect())
}

/// A received record, as plain data.
struct In {
    partition: i32,
    rec: RecordAndOffset,
}

type Fetched = Result<Vec<(i32, Vec<RecordAndOffset>)>, String>;

struct ConsumerState {
    buffer: VecDeque<In>,
    /// next offset to read, per partition
    offsets: BTreeMap<i32, i64>,
    inflight: Option<crate::native::rt::Pending<Fetched>>,
}

pub struct Consumer {
    st: Rc<RefCell<ConsumerState>>,
    inner: Arc<Inner>,
    topic: String,
    max_wait: Duration,
    max_bytes: i32,
}

fn record_value(topic: &str, i: In) -> Value {
    let r = i.rec.record;
    let text = |b: Option<Vec<u8>>| b.map(|b| Value::Str(String::from_utf8_lossy(&b).into_owned().into())).unwrap_or(Value::Nil);
    let headers: HashMap<String, Value> = r.headers.into_iter().map(|(k, v)| (k, Value::Str(String::from_utf8_lossy(&v).into_owned().into()))).collect();
    NativeObjectRef::new(KafkaRecord {
        fields: vec![
            ("topic", Value::Str(topic.to_string().into())),
            ("partition", Value::Int(i.partition as i64)),
            ("offset", Value::Int(i.rec.offset)),
            ("key", text(r.key)),
            ("value", text(r.value)),
            ("headers", Value::Map(Rc::new(RefCell::new(headers.into_iter().collect())))),
            ("timestamp", Value::Int(r.timestamp.timestamp_millis())),
        ],
    })
    .to_value()
}

impl Consumer {
    /// Starts a fetch over every partition (when none is running).
    fn fetch(&self) {
        let mut st = self.st.borrow_mut();
        if st.inflight.is_some() {
            return;
        }
        let offsets: Vec<(i32, i64)> = st.offsets.iter().map(|(p, o)| (*p, *o)).collect();
        let (inner, topic, wait, bytes) = (Arc::clone(&self.inner), self.topic.clone(), self.max_wait.as_millis() as i32, self.max_bytes);
        st.inflight = Some(crate::native::rt::spawn(async move {
            let mut tasks = Vec::new();
            for (p, off) in offsets {
                let inner = Arc::clone(&inner);
                let topic = topic.clone();
                tasks.push(tokio::spawn(async move {
                    let c = inner.partition(&topic, p).await?;
                    let (recs, _hw) = c.fetch_records(off, 1..bytes, wait).await.map_err(|e| kerr("consumer", e))?;
                    Ok::<_, String>((p, recs))
                }));
            }
            let mut out = Vec::new();
            for t in tasks {
                out.push(t.await.map_err(|e| kerr("consumer", e))??);
            }
            Ok(out)
        }));
    }

    /// Moves a finished fetch into the buffer.
    fn absorb(st: &mut ConsumerState) -> Result<(), String> {
        let Some(p) = &st.inflight else { return Ok(()) };
        let Some(r) = p.try_take() else { return Ok(()) };
        st.inflight = None;
        for (part, mut recs) in r? {
            recs.sort_by_key(|r| r.offset);
            let from = st.offsets.get(&part).copied().unwrap_or(0);
            for rec in recs.into_iter().filter(|r| r.offset >= from) {
                st.offsets.insert(part, rec.offset + 1);
                st.buffer.push_back(In { partition: part, rec });
            }
        }
        Ok(())
    }

    /// Waits until `want(buffer)` or the deadline; gives what `take` takes.
    fn wait(&self, deadline: Instant, take: fn(&mut ConsumerState) -> Option<Vec<In>>, single: bool) -> Poller {
        let st = Rc::clone(&self.st);
        let me = Consumer { st: Rc::clone(&self.st), inner: Arc::clone(&self.inner), topic: self.topic.clone(), max_wait: self.max_wait, max_bytes: self.max_bytes };
        let topic = self.topic.clone();
        Box::new(move || {
            {
                let mut s = st.borrow_mut();
                if let Err(e) = Consumer::absorb(&mut s) {
                    return Some(Err(e));
                }
                if let Some(items) = take(&mut s) {
                    let vals: Vec<Value> = items.into_iter().map(|i| record_value(&topic, i)).collect();
                    return Some(Ok(if single { vals.into_iter().next().unwrap_or(Value::Nil) } else { list(vals) }));
                }
            }
            if Instant::now() >= deadline {
                return Some(Ok(if single { Value::Nil } else { list(vec![]) }));
            }
            me.fetch();
            sched::add_timer(Instant::now() + Duration::from_millis(5));
            None
        })
    }
}

impl FoguObject for Consumer {
    fn name(&self) -> &str {
        "KafkaConsumer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        match field {
            "topic" => Some(Value::Str(self.topic.clone().into())),
            "offsets" => {
                let m: HashMap<String, Value> = self.st.borrow().offsets.iter().map(|(p, o)| (p.to_string(), Value::Int(*o))).collect();
                Some(Value::Map(Rc::new(RefCell::new(m.into_iter().collect()))))
            }
            "buffered" => Some(Value::Int(self.st.borrow().buffer.len() as i64)),
            _ => None,
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let timeout = |what: &str| -> Result<Duration, String> {
            max_args(args, 1, what)?;
            match opt_arg(args, 0) {
                Some(v) => duration_arg(v, what),
                None => Ok(Duration::from_secs(1)),
            }
        };
        match method {
            // next(timeout = 1s) → the next record, or nil
            "next" => Some(timeout("next").map(|t| self.wait(Instant::now() + t, |s| s.buffer.pop_front().map(|i| vec![i]), true))),
            // poll(timeout = 1s) → the records that arrived (maybe [])
            "poll" => Some(timeout("poll").map(|t| {
                self.wait(Instant::now() + t, |s| if s.buffer.is_empty() { None } else { Some(s.buffer.drain(..).collect()) }, false)
            })),
            _ => None,
        }
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // seek(partition, offset): read from there (drops what was buffered for it)
            "seek" => {
                max_args(args, 2, "seek")?;
                let (Some(Value::Int(p)), Some(Value::Int(o))) = (args.first(), args.get(1)) else {
                    return Err("seek(partition, offset) expects two ints".into());
                };
                let mut st = self.st.borrow_mut();
                if !st.offsets.contains_key(&(*p as i32)) {
                    return Err(format!("seek: partition {} is not read by this consumer", p));
                }
                st.offsets.insert(*p as i32, *o);
                st.buffer.retain(|i| i.partition != *p as i32);
                st.inflight = None;
                Ok(Value::Nil)
            }
            "close" => {
                let mut st = self.st.borrow_mut();
                st.inflight = None;
                st.buffer.clear();
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on KafkaConsumer ({})", other, self.methods().join(" "))),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "poll", "seek", "close"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A received record: `topic partition offset key value headers timestamp`, and `json()`.
pub struct KafkaRecord {
    fields: Vec<(&'static str, Value)>,
}

impl FoguObject for KafkaRecord {
    fn name(&self) -> &str {
        "KafkaRecord"
    }

    fn get(&self, field: &str) -> Option<Value> {
        if field == "__display__" {
            let parts: Vec<String> = self.fields.iter().filter(|(k, _)| *k != "headers").map(|(k, v)| format!("{}: {}", k, v.to_display_string())).collect();
            return Some(Value::Str(format!("{{{}}}", parts.join(", ")).into()));
        }
        self.fields.iter().find(|(k, _)| *k == field).map(|(_, v)| v.clone())
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        let value = self.get("value").unwrap_or(Value::Nil);
        match method {
            "json" => match value {
                Value::Str(s) => crate::native::json::parse_value(&s).map_err(|e| io_err!("kafka: record value is not JSON: {}", e)),
                _ => Ok(Value::Nil),
            },
            "bytes" => Ok(match value {
                Value::Str(s) => bytes_value(s.into_string().into_bytes()),
                _ => Value::Nil,
            }),
            other => Err(format!("method '{}' does not exist on KafkaRecord (json bytes)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json", "bytes"]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
