//! `fogu:rabbitmq` module — RabbitMQ / AMQP 0-9-1 client (our own,
//! `amqp.rs`, over the runtime's sockets).
//!
//! ```fogu
//! import rabbitmq from "fogu:rabbitmq"
//!
//! const mq = rabbitmq.connect("amqp://guest:guest@127.0.0.1:5672/%2f")?
//! mq.exchange("events", "topic", { durable: true })?
//! mq.queue("emails", { durable: true })?
//! mq.bind("emails", "events", "user.*")?
//! mq.publish("events", "user.created", { id: 7 }, { persistent: true })?   // waits for the broker's confirm
//!
//! const c = mq.consume("emails", { prefetch: 10 })?
//! const msg = c.next(5s)?                          // nil after 5s
//! print(msg.routingKey, msg.json().id)
//! msg.ack()                                        // or msg.nack({ requeue: false })
//! ```
//!
//! Operations run on background threads and wait cooperatively. Broker and
//! network failures are `error` values; a channel closed by an error (for
//! example a queue declared with other options) is reopened on the next call.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::amqp::{self, Conn, Delivery, Field, Inbox, Props, Table};
use crate::native::json::{parse_value, stringify_value};
use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, map, max_args, nfn_var, str_arg, thread_poller};

const TIMEOUT: Duration = Duration::from_secs(30);

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 0, connect));
    ModuleValue {
        name: "fogu:rabbitmq".into(),
        exports: RefCell::new(e),
    }
}

fn err(e: String) -> String {
    io_err!("rabbitmq: {}", e)
}

/// The connection and its main (confirm) channel, reopened after a channel error.
struct Shared {
    conn: Conn,
    main: Mutex<Option<u16>>,
}

impl Shared {
    fn channel(&self) -> Result<u16, String> {
        let mut main = self.main.lock().unwrap_or_else(|e| e.into_inner());
        let closed = {
            let mut st = self.conn.status.lock().unwrap_or_else(|e| e.into_inner());
            if !st.connected {
                return Err(err(st.error.clone().unwrap_or_else(|| "the connection is closed".into())));
            }
            let c = main.is_some_and(|m| st.closed_channels.contains(&m));
            if c {
                st.closed_channels.retain(|x| Some(*x) != *main);
            }
            c
        };
        if main.is_none() || closed {
            *main = Some(self.conn.open_channel(true, TIMEOUT).map_err(err)?);
        }
        Ok(main.expect("opened"))
    }

    /// One method on the main channel: its answer's arguments.
    fn call(&self, class: u16, m: u16, args: Vec<u8>) -> Result<amqp::Reply, String> {
        let ch = self.channel()?;
        self.conn.call(ch, class, m, &args, TIMEOUT).map_err(err)
    }
}

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "rabbitmq.connect")?;
    let url = match args.first() {
        None | Some(Value::Nil) => std::env::var("AMQP_URL").unwrap_or_else(|_| "amqp://guest:guest@127.0.0.1:5672/%2f".into()),
        Some(Value::Str(u)) => u.to_string(),
        Some(other) => return Err(format!("rabbitmq.connect expects a url, got '{}'", other.type_name())),
    };
    let u = amqp::parse_url(&url).map_err(|e| format!("rabbitmq.connect: {}", e))?;
    let timeout = match args.get(1) {
        Some(Value::Map(m)) => m.borrow().get("timeout").map(|t| duration_arg(t, "rabbitmq.connect: timeout")).transpose()?,
        _ => None,
    }
    .unwrap_or(Duration::from_secs(10));
    let shared = crate::native::rt::offload(move || -> Result<Shared, String> {
        let conn = Conn::open(&u, timeout).map_err(err)?;
        let ch = conn.open_channel(true, timeout).map_err(err)?;
        Ok(Shared { conn, main: Mutex::new(Some(ch)) })
    })??;
    Ok(NativeObjectRef::new(RabbitValue { shared: Arc::new(shared) }).to_value())
}

// ---------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------

fn opts(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn flag(o: &HashMap<String, Value>, k: &str) -> bool {
    o.get(k).is_some_and(|v| v.is_truthy())
}

fn get_s(o: &HashMap<String, Value>, k: &str) -> Option<String> {
    match o.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

fn field(v: &Value) -> Field {
    match v {
        Value::Bool(b) => Field::Bool(*b),
        Value::Int(n) => Field::Int(*n),
        Value::Float(f) => Field::Float(*f),
        Value::Str(s) => Field::Str(s.to_string()),
        Value::Duration(ns) => Field::Int(ns / 1_000_000),
        Value::Nil => Field::Void,
        Value::List(l) => Field::Array(l.borrow().iter().map(field).collect()),
        Value::Map(m) => Field::Table(table(Some(&Value::Map(m.clone())))),
        other => Field::Str(stringify_value(other)),
    }
}

/// A map → an AMQP table (keys in alphabetical order, like json.stringify).
fn table(v: Option<&Value>) -> Table {
    match v {
        Some(Value::Map(m)) => {
            let m = m.borrow();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            keys.into_iter().map(|k| (k.clone(), field(&m[k]))).collect()
        }
        _ => Vec::new(),
    }
}

fn from_field(f: &Field) -> Value {
    match f {
        Field::Bool(b) => Value::Bool(*b),
        Field::Int(n) => Value::Int(*n),
        Field::Float(x) => Value::Float(*x),
        Field::Str(s) => Value::Str(s.clone().into()),
        Field::Bytes(b) => Value::Str(String::from_utf8_lossy(b).into_owned().into()),
        Field::Timestamp(t) => Value::Int(*t as i64),
        Field::Array(a) => crate::native::util::list(a.iter().map(from_field).collect()),
        Field::Table(t) => table_value(t),
        Field::Void => Value::Nil,
    }
}

fn table_value(t: &Table) -> Value {
    map(t.iter().map(|(k, v)| (k.as_str(), from_field(v))).collect())
}

/// Payload: `str` as is, anything else as JSON (with a content type).
fn payload(v: Option<&Value>) -> (Vec<u8>, Option<&'static str>) {
    match v {
        None | Some(Value::Nil) => (Vec::new(), None),
        Some(Value::Str(s)) => (s.as_bytes().to_vec(), None),
        Some(other) => (stringify_value(other).into_bytes(), Some("application/json")),
    }
}

fn properties(o: &HashMap<String, Value>, json: Option<&str>) -> Result<Props, String> {
    let expiration = match o.get("expiration") {
        None | Some(Value::Nil) => None,
        Some(v) => Some(duration_arg(v, "publish: expiration")?.as_millis().to_string()),
    };
    Ok(Props {
        content_type: get_s(o, "contentType").or(json.map(String::from)),
        delivery_mode: flag(o, "persistent").then_some(2),
        correlation_id: get_s(o, "correlationId"),
        reply_to: get_s(o, "replyTo"),
        message_id: get_s(o, "messageId"),
        kind: get_s(o, "type"),
        app_id: get_s(o, "appId"),
        expiration,
        priority: match o.get("priority") {
            Some(Value::Int(n)) => Some((*n).clamp(0, 255) as u8),
            _ => None,
        },
        headers: o.contains_key("headers").then(|| table(o.get("headers"))),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------
// Connection object
// ---------------------------------------------------------------------

pub struct RabbitValue {
    shared: Arc<Shared>,
}

impl FoguObject for RabbitValue {
    fn name(&self) -> &str {
        "RabbitMQ"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "connected" => Value::Bool(self.shared.conn.status.lock().map(|s| s.connected).unwrap_or(false)),
            _ => return None,
        })
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let sh = Arc::clone(&self.shared);
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // exchange(name, kind = "direct", { durable, autoDelete, internal, passive, arguments })
                "exchange" => {
                    max_args(args, 3, "exchange")?;
                    let name = str_arg(args, 0, "exchange")?;
                    let kind = match args.get(1) {
                        None | Some(Value::Nil) => "direct".to_string(),
                        Some(Value::Str(k)) => k.to_string(),
                        Some(other) => return Err(format!("exchange: kind must be a str, got '{}'", other.type_name())),
                    };
                    let o = opts(args, 2, "exchange")?;
                    let a = amqp::enc()
                        .u16(0)
                        .short(&name)
                        .short(&kind)
                        .bits(&[flag(&o, "passive"), flag(&o, "durable"), flag(&o, "autoDelete"), flag(&o, "internal"), false])
                        .table(&table(o.get("arguments")))
                        .done();
                    thread_poller(move || sh.call(40, 10, a).map(|_| ()), |_| Ok(Value::Nil))
                }
                // queue(name = "", { durable, exclusive, autoDelete, passive, ttl, maxLength, deadLetter, arguments })
                //   → { name, messages, consumers } ("" gives a server-named queue)
                "queue" => {
                    max_args(args, 2, "queue")?;
                    let name = match args.first() {
                        None | Some(Value::Nil) => String::new(),
                        _ => str_arg(args, 0, "queue")?,
                    };
                    let o = opts(args, 1, "queue")?;
                    let mut t = table(o.get("arguments"));
                    if let Some(v) = o.get("ttl").filter(|v| !matches!(v, Value::Nil)) {
                        t.push(("x-message-ttl".into(), Field::Int(duration_arg(v, "queue: ttl")?.as_millis() as i64)));
                    }
                    if let Some(Value::Int(n)) = o.get("maxLength") {
                        t.push(("x-max-length".into(), Field::Int(*n)));
                    }
                    if let Some(dl) = get_s(&o, "deadLetter") {
                        t.push(("x-dead-letter-exchange".into(), Field::Str(dl)));
                    }
                    let a = amqp::enc()
                        .u16(0)
                        .short(&name)
                        .bits(&[flag(&o, "passive"), flag(&o, "durable"), flag(&o, "exclusive"), flag(&o, "autoDelete"), false])
                        .table(&t)
                        .done();
                    thread_poller(
                        move || {
                            let r = sh.call(50, 10, a)?;
                            let mut d = amqp::dec(&r.args);
                            let n = d.short().map_err(err)?;
                            Ok((n, d.u32().unwrap_or(0), d.u32().unwrap_or(0)))
                        },
                        |(n, m, c)| Ok(map(vec![("name", Value::Str(n.into())), ("messages", Value::Int(m as i64)), ("consumers", Value::Int(c as i64))])),
                    )
                }
                // bind(queue, exchange, routingKey = "", { arguments }) · unbind(...)
                "bind" | "unbind" => {
                    max_args(args, 4, method)?;
                    let queue = str_arg(args, 0, method)?;
                    let exchange = str_arg(args, 1, method)?;
                    let key = match args.get(2) {
                        Some(Value::Str(k)) => k.clone(),
                        _ => String::new().into(),
                    };
                    let t = table(opts(args, 3, method)?.get("arguments"));
                    let bind = method == "bind";
                    let a = if bind {
                        amqp::enc().u16(0).short(&queue).short(&exchange).short(&key).bits(&[false]).table(&t).done()
                    } else {
                        amqp::enc().u16(0).short(&queue).short(&exchange).short(&key).table(&t).done()
                    };
                    thread_poller(move || sh.call(50, if bind { 20 } else { 50 }, a).map(|_| ()), |_| Ok(Value::Nil))
                }
                // publish(exchange, routingKey, data, { persistent, headers, contentType, correlationId,
                //   replyTo, messageId, type, appId, expiration, priority, mandatory }) — waits for the confirm
                // send(queue, data, options) — through the default exchange
                "publish" | "send" => {
                    let (exchange, key, data_i) = if method == "send" {
                        max_args(args, 3, "send")?;
                        (String::new(), str_arg(args, 0, "send")?, 1)
                    } else {
                        max_args(args, 4, "publish")?;
                        (str_arg(args, 0, "publish")?, str_arg(args, 1, "publish")?, 2)
                    };
                    let (body, json) = payload(args.get(data_i));
                    let o = opts(args, data_i + 1, method)?;
                    let props = properties(&o, json)?;
                    let mandatory = flag(&o, "mandatory");
                    thread_poller(
                        move || {
                            let ch = sh.channel()?;
                            sh.conn.publish(ch, &exchange, &key, mandatory, &props, &body, TIMEOUT).map_err(err)
                        },
                        |_| Ok(Value::Nil),
                    )
                }
                // get(queue, { noAck }) → Message or nil
                "get" => {
                    max_args(args, 2, "get")?;
                    let queue = str_arg(args, 0, "get")?;
                    let no_ack = flag(&opts(args, 1, "get")?, "noAck");
                    let a = amqp::enc().u16(0).short(&queue).bits(&[no_ack]).done();
                    let sh2 = Arc::clone(&sh);
                    thread_poller(move || Ok(sh.call(60, 70, a)?.content), move |d| Ok(d.map_or(Value::Nil, |d| delivery_value(d, Arc::clone(&sh2), no_ack))))
                }
                // consume(queue, { prefetch = 10, noAck, exclusive, tag }) → Consumer
                "consume" => {
                    max_args(args, 2, "consume")?;
                    let queue = str_arg(args, 0, "consume")?;
                    let o = opts(args, 1, "consume")?;
                    let prefetch = match o.get("prefetch") {
                        Some(Value::Int(n)) if *n >= 0 => (*n).min(65535) as u16,
                        _ => 10,
                    };
                    let no_ack = flag(&o, "noAck");
                    let exclusive = flag(&o, "exclusive");
                    let tag = get_s(&o, "tag").unwrap_or_else(|| format!("fogu-{}", uuid::Uuid::new_v4().simple()));
                    thread_poller(move || Consumer::start(sh, &queue, prefetch, no_ack, exclusive, tag), |c| Ok(NativeObjectRef::new(c).to_value()))
                }
                // purge(queue) → removed count · deleteQueue(name) → removed count · deleteExchange(name)
                "purge" | "deleteQueue" | "deleteExchange" => {
                    max_args(args, 1, method)?;
                    let name = str_arg(args, 0, method)?;
                    let (class, m, a) = match method {
                        "purge" => (50, 30, amqp::enc().u16(0).short(&name).bits(&[false]).done()),
                        "deleteQueue" => (50, 40, amqp::enc().u16(0).short(&name).bits(&[false, false, false]).done()),
                        _ => (40, 20, amqp::enc().u16(0).short(&name).bits(&[false, false]).done()),
                    };
                    thread_poller(
                        move || {
                            let r = sh.call(class, m, a)?;
                            Ok(if class == 50 { amqp::dec(&r.args).u32().unwrap_or(0) as i64 } else { 0 })
                        },
                        |n| Ok(Value::Int(n)),
                    )
                }
                "close" => {
                    max_args(args, 0, "close")?;
                    thread_poller(move || sh.conn.close(Duration::from_secs(5)).map_err(err), |_| Ok(Value::Nil))
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
        Err(format!(
            "method '{}' does not exist on RabbitMQ (exchange queue bind unbind publish send get consume purge deleteQueue deleteExchange close)",
            method
        ))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["exchange", "queue", "bind", "unbind", "publish", "send", "get", "consume", "purge", "deleteQueue", "deleteExchange", "close"]
    }
}

// ---------------------------------------------------------------------
// Consumer and messages
// ---------------------------------------------------------------------

pub struct Consumer {
    inbox: Arc<Mutex<Inbox>>,
    shared: Arc<Shared>,
    channel: u16,
    tag: String,
    queue: String,
    no_ack: bool,
}

impl Consumer {
    fn start(sh: Arc<Shared>, queue: &str, prefetch: u16, no_ack: bool, exclusive: bool, tag: String) -> Result<Consumer, String> {
        // a channel of its own, so the prefetch applies to this consumer
        let ch = sh.conn.open_channel(false, TIMEOUT).map_err(err)?;
        if prefetch > 0 {
            sh.conn.call(ch, 60, 10, &amqp::enc().u32(0).u16(prefetch).bits(&[false]).done(), TIMEOUT).map_err(err)?;
        }
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        sh.conn.register(&tag, ch, Arc::clone(&inbox)).map_err(err)?;
        let a = amqp::enc().u16(0).short(queue).short(&tag).bits(&[false, no_ack, exclusive, false]).table(&Vec::new()).done();
        if let Err(e) = sh.conn.call(ch, 60, 20, &a, TIMEOUT) {
            sh.conn.close_channel(ch, Duration::from_secs(2));
            return Err(err(e));
        }
        Ok(Consumer { inbox, shared: sh, channel: ch, tag, queue: queue.to_string(), no_ack })
    }
}

impl FoguObject for Consumer {
    fn name(&self) -> &str {
        "RabbitConsumer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "tag" => Value::Str(self.tag.clone().into()),
            "queue" => Value::Str(self.queue.clone().into()),
            "pending" => Value::Int(self.inbox.lock().map(|b| b.deliveries.len()).unwrap_or(0) as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => {
                max_args(args, 0, "tryNext")?;
                let d = self.inbox.lock().unwrap_or_else(|e| e.into_inner()).deliveries.pop_front();
                Ok(d.map_or(Value::Nil, |d| delivery_value(d, Arc::clone(&self.shared), self.no_ack)))
            }
            "cancel" | "close" => {
                max_args(args, 0, method)?;
                let (sh, ch, tag) = (Arc::clone(&self.shared), self.channel, self.tag.clone());
                std::thread::spawn(move || {
                    let _ = sh.conn.call(ch, 60, 30, &amqp::enc().short(&tag).bits(&[false]).done(), Duration::from_secs(5));
                    sh.conn.close_channel(ch, Duration::from_secs(5));
                });
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on RabbitConsumer (next tryNext cancel)", other)),
        }
    }

    /// `next(timeout?)` → Message, nil on timeout.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match (max_args(args, 1, "next"), args.first()) {
            (Err(e), _) => return Some(Err(e)),
            (_, None | Some(Value::Nil)) => None,
            (_, Some(v)) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let (inbox, sh, no_ack) = (Arc::clone(&self.inbox), Arc::clone(&self.shared), self.no_ack);
        Some(Ok(Box::new(move || {
            let mut b = inbox.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(d) = b.deliveries.pop_front() {
                return Some(Ok(delivery_value(d, Arc::clone(&sh), no_ack)));
            }
            if let Some(e) = &b.error {
                return Some(Err(io_err!("rabbitmq: {}", e)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(2));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "cancel"]
    }
}

fn delivery_value(d: Delivery, shared: Arc<Shared>, no_ack: bool) -> Value {
    NativeObjectRef::new(MessageValue { d, shared, settled: no_ack }).to_value()
}

pub struct MessageValue {
    d: Delivery,
    shared: Arc<Shared>,
    settled: bool,
}

fn short(s: &Option<String>) -> Value {
    s.as_ref().map_or(Value::Nil, |s| Value::Str(s.clone().into()))
}

impl FoguObject for MessageValue {
    fn name(&self) -> &str {
        "RabbitMessage"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let p = &self.d.props;
        Some(match field {
            "data" | "body" => Value::Str(String::from_utf8_lossy(&self.d.body).into_owned().into()),
            "routingKey" => Value::Str(self.d.routing_key.clone().into()),
            "exchange" => Value::Str(self.d.exchange.clone().into()),
            "redelivered" => Value::Bool(self.d.redelivered),
            "deliveryTag" => Value::Int(self.d.delivery_tag as i64),
            "contentType" => short(&p.content_type),
            "correlationId" => short(&p.correlation_id),
            "replyTo" => short(&p.reply_to),
            "messageId" => short(&p.message_id),
            "type" => short(&p.kind),
            "appId" => short(&p.app_id),
            "headers" => p.headers.as_ref().map_or_else(|| map(vec![]), table_value),
            "persistent" => Value::Bool(p.delivery_mode == Some(2)),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        if method == "json" {
            return parse_value(&String::from_utf8_lossy(&self.d.body));
        }
        if !matches!(method, "ack" | "nack" | "reject") {
            return Err(format!("method '{}' does not exist on RabbitMessage (json ack nack reject)", method));
        }
        max_args(args, 1, method)?;
        if self.settled {
            return Err(format!("{}: the message was already acknowledged (or consumed with noAck)", method));
        }
        let requeue = match args.first() {
            Some(Value::Map(m)) => m.borrow().get("requeue").map_or(method == "nack", |v| v.is_truthy()),
            _ => method == "nack",
        };
        let (ch, tag) = (self.d.channel, self.d.delivery_tag);
        let r = match method {
            "ack" => self.shared.conn.cast(ch, 60, 80, &amqp::enc().u64(tag).bits(&[false]).done()),
            "nack" => self.shared.conn.cast(ch, 60, 120, &amqp::enc().u64(tag).bits(&[false, requeue]).done()),
            _ => self.shared.conn.cast(ch, 60, 90, &amqp::enc().u64(tag).bits(&[requeue]).done()),
        };
        self.settled = true;
        r.map(|_| Value::Nil).map_err(err)
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["json", "ack", "nack", "reject"]
    }
}
