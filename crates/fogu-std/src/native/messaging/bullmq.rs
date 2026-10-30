//! `fogu:bullmq` module — BullMQ job queues on Redis (on the `bullmq-rs`
//! crate, wire-compatible with BullMQ for Node.js v5: Fogu and Node workers
//! and producers can share the same queues, and Bull Board sees them).
//!
//! ```fogu
//! import bullmq from "fogu:bullmq"
//!
//! const q = bullmq.queue("emails", { connection: "redis://127.0.0.1:6379" })?
//! q.add("welcome", { to: "ana@x.com" }, { attempts: 3, backoff: { type: "exponential", delay: 1s } })?
//!
//! const w = bullmq.worker("emails", { concurrency: 4 })?
//! while true {
//!     const job = w.next()?                    // waits (other tasks keep running)
//!     print(job.name, job.data.to)
//!     job.progress(50)
//!     job.complete({ sent: true })             // or job.fail("smtp down") → retried
//! }
//! ```
//!
//! The worker of `bullmq-rs` owns the jobs (locks, stalled-job recovery,
//! retries with backoff, concurrency, rate limits); each job is handed to the
//! program and finished when it calls `complete` or `fail`. Network and Redis
//! failures are `error` values.

use std::collections::VecDeque;
use ahash::AHashMap as HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bullmq_rs::{
    BackoffStrategy, DeduplicationOptions, Job, JobOptions, JobSchedulerTemplate, JobState, Queue, QueueBuilder,
    QueueEvent, QueueEventsBuilder, RateLimiterOptions, RedisConnection, RepeatOptions, WorkerBuilder, WorkerHandle,
};
use serde_json::{json, Value as J};

use crate::runtime::sched;
use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, json_to_value, list, max_args, nfn_var, str_arg, thread_poller};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("queue".into(), nfn_var("queue", 1, queue));
    e.insert("worker".into(), nfn_var("worker", 1, worker));
    e.insert("events".into(), nfn_var("events", 1, events));
    ModuleValue {
        name: "fogu:bullmq".into(),
        exports: std::cell::RefCell::new(e),
    }
}

// ---------------------------------------------------------------------
// Shared runtime and options
// ---------------------------------------------------------------------

/// One tokio runtime for every queue, worker and event stream.
fn runtime() -> &'static tokio::runtime::Runtime {
    crate::native::rt::tokio()
}

fn err(e: impl std::fmt::Display) -> String {
    io_err!("bullmq: {}", e)
}

fn to_json(v: &Value) -> J {
    serde_json::from_str(&crate::native::json::stringify_value(v)).unwrap_or(J::Null)
}

fn opts_map(args: &[Value], i: usize, what: &str) -> Result<HashMap<String, Value>, String> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(HashMap::new()),
        Some(Value::Map(m)) => Ok(m.borrow().clone()),
        Some(other) => Err(format!("{}: options must be a map, got '{}'", what, other.type_name())),
    }
}

fn get_s(o: &HashMap<String, Value>, k: &str) -> Option<String> {
    match o.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

fn dur(o: &HashMap<String, Value>, k: &str, what: &str) -> Result<Option<Duration>, String> {
    match o.get(k) {
        None | Some(Value::Nil) => Ok(None),
        Some(v) => duration_arg(v, &format!("{}: {}", what, k)).map(Some),
    }
}

fn int(o: &HashMap<String, Value>, k: &str) -> Option<u64> {
    match o.get(k) {
        Some(Value::Int(n)) if *n >= 0 => Some(*n as u64),
        _ => None,
    }
}

/// `{ connection | url, prefix }` → (connection, prefix). The server is
/// probed first: an unreachable Redis is an error in seconds, not after the
/// client's long reconnection loop.
fn connection(o: &HashMap<String, Value>) -> Result<(RedisConnection, String), String> {
    let url = get_s(o, "connection")
        .or_else(|| get_s(o, "url"))
        .or_else(|| std::env::var("REDIS_URL").ok())
        .unwrap_or_else(|| "redis://127.0.0.1:6379".into());
    let client = ::redis::Client::open(url.as_str()).map_err(err)?;
    let (host, port) = match client.get_connection_info().addr() {
        ::redis::ConnectionAddr::Tcp(h, p) => (h.clone(), *p),
        ::redis::ConnectionAddr::TcpTls { host, port, .. } => (host.clone(), *port),
        _ => return Ok((RedisConnection::new(url), get_s(o, "prefix").unwrap_or_else(|| "bull".into()))),
    };
    let addrs = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port)).map_err(|e| err(format!("{}:{}: {}", host, port, e)))?;
    let mut last = format!("{}:{}: no address", host, port);
    let reachable = addrs.into_iter().any(|a| match std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
        Ok(_) => true,
        Err(e) => {
            last = format!("{}:{}: {}", host, port, e);
            false
        }
    });
    if !reachable {
        return Err(err(last));
    }
    Ok((RedisConnection::new(url), get_s(o, "prefix").unwrap_or_else(|| "bull".into())))
}

/// Job options in BullMQ's names: `{ delay, attempts, backoff: { type, delay, max },
/// priority, jobId, ttl, deduplication: { id, ttl } }`.
fn job_options(o: &HashMap<String, Value>, what: &str) -> Result<Option<JobOptions>, String> {
    if o.is_empty() {
        return Ok(None);
    }
    let backoff = match o.get("backoff") {
        None | Some(Value::Nil) => None,
        Some(Value::Int(ms)) => Some(BackoffStrategy::Fixed { delay: Duration::from_millis(*ms as u64) }),
        Some(Value::Duration(ns)) => Some(BackoffStrategy::Fixed { delay: Duration::from_nanos(*ns as u64) }),
        Some(Value::Map(b)) => {
            let b = b.borrow().clone();
            let delay = dur(&b, "delay", what)?.unwrap_or(Duration::from_secs(1));
            match get_s(&b, "type").as_deref() {
                Some("exponential") => Some(BackoffStrategy::Exponential {
                    base: delay,
                    max: dur(&b, "max", what)?.unwrap_or(Duration::from_secs(3600)),
                }),
                Some("fixed") | None => Some(BackoffStrategy::Fixed { delay }),
                Some(other) => return Err(format!("{}: backoff type must be fixed or exponential, got '{}'", what, other)),
            }
        }
        Some(other) => return Err(format!("{}: backoff must be a map, got '{}'", what, other.type_name())),
    };
    let deduplication = match o.get("deduplication") {
        Some(Value::Map(d)) => {
            let d = d.borrow().clone();
            Some(DeduplicationOptions {
                id: get_s(&d, "id").ok_or(format!("{}: deduplication needs an 'id'", what))?,
                ttl: dur(&d, "ttl", what)?,
            })
        }
        _ => None,
    };
    Ok(Some(JobOptions {
        priority: int(o, "priority").map(|p| p as u32),
        delay: dur(o, "delay", what)?,
        attempts: int(o, "attempts").map(|a| a as u32),
        backoff,
        ttl: dur(o, "ttl", what)?,
        job_id: get_s(o, "jobId").or_else(|| int(o, "jobId").map(|n| n.to_string())),
        deduplication,
        ..Default::default()
    }))
}

fn state_name(s: &JobState) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| format!("{:?}", s).to_lowercase())
}

fn parse_state(s: &str) -> Result<JobState, String> {
    serde_json::from_value(J::String(s.to_string())).map_err(|_| {
        format!("bullmq: unknown job state '{}' (wait active delayed prioritized completed failed paused waiting-children)", s)
    })
}

/// A job as a Fogu map.
fn job_json(j: &Job<J>) -> J {
    json!({
        "id": j.id,
        "name": j.name,
        "data": j.data,
        "state": state_name(&j.state),
        "timestamp": j.timestamp,
        "priority": j.priority,
        "delay": j.delay,
        "attemptsMade": j.attempts_made,
        "progress": j.progress,
        "processedOn": j.processed_on,
        "finishedOn": j.finished_on,
        "failedReason": j.failed_reason,
        "returnvalue": j.return_value,
        "opts": serde_json::to_value(&j.opts).unwrap_or(J::Null),
    })
}

// ---------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------

fn queue(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "bullmq.queue")?;
    let name = str_arg(args, 0, "bullmq.queue")?;
    let o = opts_map(args, 1, "bullmq.queue")?;
    let (conn, prefix) = connection(&o)?;
    let url = conn.url().to_string();
    let q = runtime()
        .block_on(QueueBuilder::new(name.clone()).connection(conn).prefix(prefix.clone()).build::<J>())
        .map_err(err)?;
    Ok(NativeObjectRef::new(QueueValue { q: Arc::new(q), name, url, prefix }).to_value())
}

pub struct QueueValue {
    q: Arc<Queue<J>>,
    name: String,
    url: String,
    prefix: String,
}

/// Runs an async queue operation on the shared runtime, from a worker thread.
fn on_queue<T: Send + 'static, F, Fut>(q: &Arc<Queue<J>>, f: F, finish: impl FnOnce(T) -> Result<Value, String> + 'static) -> Poller
where
    F: FnOnce(Arc<Queue<J>>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    let q = Arc::clone(q);
    thread_poller(move || runtime().block_on(f(q)), finish)
}

impl FoguObject for QueueValue {
    fn name(&self) -> &str {
        "BullQueue"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "name" => Value::Str(self.name.clone().into()),
            "prefix" => Value::Str(self.prefix.clone().into()),
            "url" => Value::Str(self.url.clone().into()),
            _ => return None,
        })
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let q = &self.q;
        let r = (|| -> Result<Option<Poller>, String> {
            Ok(Some(match method {
                // add(name, data, opts?) → job
                "add" => {
                    max_args(args, 3, "add")?;
                    let name = str_arg(args, 0, "add")?;
                    let data = args.get(1).map(to_json).unwrap_or(J::Null);
                    let opts = job_options(&opts_map(args, 2, "add")?, "add")?;
                    on_queue(q, move |q| async move { q.add(&name, data, opts).await.map_err(err) }, |j| Ok(json_to_value(&job_json(&j))))
                }
                // addBulk([{ name, data, opts }]) → jobs
                "addBulk" => {
                    max_args(args, 1, "addBulk")?;
                    let items = match args.first() {
                        Some(Value::List(l)) => l.borrow().clone(),
                        _ => return Err("addBulk expects a list of { name, data, opts }".into()),
                    };
                    let mut jobs = Vec::new();
                    for it in &items {
                        let m = match it {
                            Value::Map(m) => m.borrow().clone(),
                            _ => return Err("addBulk: each job is a map { name, data, opts }".into()),
                        };
                        let name = get_s(&m, "name").ok_or("addBulk: a job needs a 'name'")?;
                        let data = m.get("data").map(to_json).unwrap_or(J::Null);
                        let opts = match m.get("opts") {
                            Some(Value::Map(o)) => job_options(&o.borrow().clone(), "addBulk")?,
                            _ => None,
                        };
                        jobs.push((name, data, opts));
                    }
                    on_queue(q, move |q| async move { q.add_bulk(jobs).await.map_err(err) }, |js| Ok(list(js.iter().map(|j| json_to_value(&job_json(j))).collect())))
                }
                "getJob" => {
                    max_args(args, 1, "getJob")?;
                    let id = match args.first() {
                        Some(Value::Int(n)) => n.to_string(),
                        _ => str_arg(args, 0, "getJob")?,
                    };
                    on_queue(
                        q,
                        move |q| async move {
                            let mut j = q.get_job(&id).await.map_err(err)?;
                            // the stored hash has no state: ask Redis
                            if let Some(j) = j.as_mut() {
                                j.get_state().await.map_err(err)?;
                            }
                            Ok(j)
                        },
                        |j| Ok(j.map_or(Value::Nil, |j| json_to_value(&job_json(&j)))))
                }
                // getJobs(states, start = 0, end = -1, asc = false)
                "getJobs" => {
                    max_args(args, 4, "getJobs")?;
                    let states: Vec<JobState> = match args.first() {
                        Some(Value::Str(s)) => vec![parse_state(s)?],
                        Some(Value::List(l)) => l.borrow().iter().map(|v| parse_state(&v.to_display_string())).collect::<Result<_, _>>()?,
                        _ => return Err("getJobs expects a state or a list of states".into()),
                    };
                    let n = |i: usize, d: i64| match args.get(i) {
                        Some(Value::Int(n)) => *n,
                        _ => d,
                    };
                    let (start, end) = (n(1, 0), n(2, -1));
                    let asc = args.get(3).is_some_and(|v| v.is_truthy());
                    on_queue(
                        q,
                        move |q| async move {
                            let mut js = q.get_jobs(&states, start, end, asc).await.map_err(err)?;
                            for j in js.iter_mut() {
                                j.get_state().await.map_err(err)?;
                            }
                            Ok(js)
                        },
                        |js| Ok(list(js.iter().map(|j| json_to_value(&job_json(j))).collect())))
                }
                // counts() → { wait, active, delayed, completed, failed, … }
                "counts" | "getJobCounts" => on_queue(
                    q,
                    |q| async move { q.get_job_counts().await.map_err(err) },
                    |c| {
                        let mut m = serde_json::Map::new();
                        for (s, n) in c {
                            m.insert(state_name(&s), J::from(n));
                        }
                        Ok(json_to_value(&J::Object(m)))
                    },
                ),
                "count" => on_queue(q, |q| async move { q.count().await.map_err(err) }, |n| Ok(Value::Int(n as i64))),
                "pause" => on_queue(q, |q| async move { q.pause().await.map_err(err) }, |_| Ok(Value::Nil)),
                "resume" => on_queue(q, |q| async move { q.resume().await.map_err(err) }, |_| Ok(Value::Nil)),
                "isPaused" => on_queue(q, |q| async move { q.is_paused().await.map_err(err) }, |b| Ok(Value::Bool(b))),
                "drain" => on_queue(q, |q| async move { q.drain().await.map_err(err) }, |_| Ok(Value::Nil)),
                "remove" => {
                    let id = match args.first() {
                        Some(Value::Int(n)) => n.to_string(),
                        _ => str_arg(args, 0, "remove")?,
                    };
                    on_queue(q, move |q| async move { q.remove(&id).await.map_err(err) }, |_| Ok(Value::Nil))
                }
                // clean(grace, limit, state = "completed") → removed ids
                "clean" => {
                    max_args(args, 3, "clean")?;
                    let grace = duration_arg(args.first().unwrap_or(&Value::Int(0)), "clean: grace")?;
                    let limit = match args.get(1) {
                        Some(Value::Int(n)) if *n >= 0 => *n as u64,
                        _ => 1000,
                    };
                    let state = parse_state(&args.get(2).map(|v| v.to_display_string()).unwrap_or_else(|| "completed".into()))?;
                    on_queue(q, move |q| async move { q.clean(grace, limit, state).await.map_err(err) }, |ids| Ok(crate::native::util::str_list(ids)))
                }
                "obliterate" => {
                    let force = matches!(args.first(), Some(Value::Map(m)) if m.borrow().get("force").is_some_and(|v| v.is_truthy()));
                    on_queue(q, move |q| async move { q.obliterate(force).await.map_err(err) }, |_| Ok(Value::Nil))
                }
                // upsertJobScheduler(id, { every | pattern, tz, limit, immediately }, { name, data, opts })
                "upsertJobScheduler" => {
                    max_args(args, 3, "upsertJobScheduler")?;
                    let id = str_arg(args, 0, "upsertJobScheduler")?;
                    let r = opts_map(args, 1, "upsertJobScheduler")?;
                    let t = opts_map(args, 2, "upsertJobScheduler")?;
                    let repeat = RepeatOptions {
                        pattern: get_s(&r, "pattern"),
                        every: dur(&r, "every", "upsertJobScheduler")?,
                        limit: int(&r, "limit"),
                        tz: get_s(&r, "tz"),
                        immediately: r.get("immediately").is_some_and(|v| v.is_truthy()),
                        ..Default::default()
                    };
                    let template = JobSchedulerTemplate {
                        name: get_s(&t, "name"),
                        data: t.get("data").map(to_json),
                        opts: match t.get("opts") {
                            Some(Value::Map(o)) => job_options(&o.borrow().clone(), "upsertJobScheduler")?,
                            _ => None,
                        },
                    };
                    on_queue(q, move |q| async move { q.upsert_job_scheduler(&id, repeat, template).await.map_err(err) }, |j| Ok(json_to_value(&job_json(&j))))
                }
                "removeJobScheduler" => {
                    let id = str_arg(args, 0, "removeJobScheduler")?;
                    on_queue(q, move |q| async move { q.remove_job_scheduler(&id).await.map_err(err) }, |b| Ok(Value::Bool(b)))
                }
                "getJobLogs" => {
                    let id = str_arg(args, 0, "getJobLogs")?;
                    on_queue(q, move |q| async move { q.get_logs(&id, 0, -1).await.map_err(err) }, |l| Ok(crate::native::util::str_list(l)))
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
            other => Err(format!(
                "method '{}' does not exist on BullQueue (add addBulk getJob getJobs counts count pause resume isPaused drain remove clean obliterate upsertJobScheduler removeJobScheduler getJobLogs close)",
                other
            )),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec![
            "add", "addBulk", "getJob", "getJobs", "counts", "count", "pause", "resume", "isPaused", "drain", "remove",
            "clean", "obliterate", "upsertJobScheduler", "removeJobScheduler", "getJobLogs", "close",
        ]
    }
}

// ---------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------

enum Cmd {
    Progress(J),
    Log(String),
    Complete(J),
    Fail(String),
}

struct Delivery {
    job: J,
    cmds: tokio::sync::mpsc::UnboundedSender<Cmd>,
}

#[derive(Default)]
struct Inbox {
    jobs: VecDeque<Delivery>,
    error: Option<String>,
    closed: bool,
}

/// Stores the return value once the worker moved the job to completed (the
/// worker itself records `{}`). Only when the job hash still exists.
const SET_RETURN: &str = r#"
if redis.call('EXISTS', KEYS[1]) == 1 and redis.call('HEXISTS', KEYS[1], 'finishedOn') == 1 then
  redis.call('HSET', KEYS[1], 'returnvalue', ARGV[1])
  return 1
end
return 0
"#;

async fn record_return(url: String, key: String, value: J) {
    let Ok(client) = ::redis::Client::open(url.as_str()) else { return };
    let Ok(mut conn) = client.get_multiplexed_async_connection().await else { return };
    let script = ::redis::Script::new(SET_RETURN);
    // the worker finishes the job right after the handler returns
    for _ in 0..50 {
        let done: i64 = script.key(&key).arg(value.to_string()).invoke_async(&mut conn).await.unwrap_or(0);
        if done == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn worker(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "bullmq.worker")?;
    let name = str_arg(args, 0, "bullmq.worker")?;
    let o = opts_map(args, 1, "bullmq.worker")?;
    let (conn, prefix) = connection(&o)?;
    let url = conn.url().to_string();
    let mut b = WorkerBuilder::new(name.clone())
        .connection(conn)
        .prefix(prefix.clone())
        .concurrency(int(&o, "concurrency").unwrap_or(1).max(1) as usize);
    if let Some(d) = dur(&o, "lockDuration", "bullmq.worker")? {
        b = b.lock_duration(d);
    }
    if let Some(n) = int(&o, "maxStalledCount") {
        b = b.max_stalled_count(n as u32);
    }
    if let Some(Value::Map(l)) = o.get("limiter") {
        let l = l.borrow().clone();
        b = b.limiter(RateLimiterOptions {
            max: int(&l, "max").ok_or("bullmq.worker: limiter needs 'max'")?,
            duration: dur(&l, "duration", "bullmq.worker")?.ok_or("bullmq.worker: limiter needs 'duration'")?,
        });
    }
    let w = b.build::<J>();
    let inbox = Arc::new(Mutex::new(Inbox::default()));
    let handler_inbox = Arc::clone(&inbox);
    let (url2, prefix2, name2) = (url.clone(), prefix.clone(), name.clone());
    let handle = runtime()
        .block_on(w.start(move |mut job: Job<J>| {
            let inbox = Arc::clone(&handler_inbox);
            let (url, key) = (url2.clone(), format!("{}:{}:{}", prefix2, name2, job.id));
            async move {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                inbox.lock().unwrap().jobs.push_back(Delivery { job: job_json(&job), cmds: tx });
                loop {
                    match rx.recv().await {
                        Some(Cmd::Progress(p)) => {
                            let _ = job.update_progress(p).await;
                        }
                        Some(Cmd::Log(l)) => {
                            let _ = job.log(&l).await;
                        }
                        Some(Cmd::Complete(v)) => {
                            if !v.is_null() {
                                tokio::spawn(record_return(url, key, v));
                            }
                            return Ok(());
                        }
                        Some(Cmd::Fail(reason)) => return Err(reason.into()),
                        // the program dropped the job without finishing it
                        None => return Err("the job was not completed (dropped by the program)".into()),
                    }
                }
            }
        }))
        .map_err(err)?;
    Ok(NativeObjectRef::new(WorkerValue { inbox, handle: Some(handle), name }).to_value())
}

pub struct WorkerValue {
    inbox: Arc<Mutex<Inbox>>,
    handle: Option<WorkerHandle>,
    name: String,
}

fn job_value(d: Delivery) -> Value {
    NativeObjectRef::new(JobValue { job: d.job, cmds: Some(d.cmds) }).to_value()
}

impl FoguObject for WorkerValue {
    fn name(&self) -> &str {
        "BullWorker"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "name" => Value::Str(self.name.clone().into()),
            "running" => Value::Bool(self.handle.as_ref().is_some_and(|h| h.is_running())),
            "paused" => Value::Bool(self.handle.as_ref().is_some_and(|h| h.is_paused())),
            "pending" => Value::Int(self.inbox.lock().unwrap().jobs.len() as i64),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => Ok(self.inbox.lock().unwrap().jobs.pop_front().map_or(Value::Nil, job_value)),
            "pause" => {
                if let Some(h) = &self.handle {
                    runtime().block_on(h.pause(true));
                }
                Ok(Value::Nil)
            }
            "resume" => {
                if let Some(h) = &self.handle {
                    h.resume();
                }
                Ok(Value::Nil)
            }
            "close" => {
                max_args(args, 0, "close")?;
                self.inbox.lock().unwrap().closed = true;
                if let Some(h) = self.handle.take() {
                    h.shutdown();
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on BullWorker (next tryNext pause resume close)", other)),
        }
    }

    /// `next(timeout?)` → Job, nil on timeout or after `close()`.
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
        let inbox = Arc::clone(&self.inbox);
        Some(Ok(Box::new(move || {
            let mut b = inbox.lock().unwrap();
            if let Some(d) = b.jobs.pop_front() {
                return Some(Ok(job_value(d)));
            }
            if let Some(e) = b.error.take() {
                return Some(Err(err(e)));
            }
            if b.closed || deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(5));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext", "pause", "resume", "close"]
    }
}

/// A job being processed: `complete(value?)`, `fail(reason)`, `progress(v)`, `log(text)`.
pub struct JobValue {
    job: J,
    cmds: Option<tokio::sync::mpsc::UnboundedSender<Cmd>>,
}

impl Drop for JobValue {
    fn drop(&mut self) {
        // an unfinished job fails (and may be retried) instead of hanging
        if let Some(tx) = self.cmds.take() {
            let _ = tx.send(Cmd::Fail("the job was not completed (dropped by the program)".into()));
        }
    }
}

impl FoguObject for JobValue {
    fn name(&self) -> &str {
        "BullJob"
    }

    fn get(&self, field: &str) -> Option<Value> {
        if field == "done" {
            return Some(Value::Bool(self.cmds.is_none()));
        }
        // methods win over fields of the same name (`progress`)
        if self.methods().contains(&field) {
            return None;
        }
        self.job.get(field).map(json_to_value)
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        let send = |s: &mut Self, cmd: Cmd, finish: bool| -> Result<Value, String> {
            let tx = if finish { s.cmds.take() } else { s.cmds.clone() };
            let tx = tx.ok_or_else(|| format!("{}: the job was already completed or failed", method))?;
            tx.send(cmd).map_err(|_| err("the worker was closed"))?;
            Ok(Value::Nil)
        };
        match method {
            "complete" => {
                max_args(args, 1, "complete")?;
                send(self, Cmd::Complete(args.first().map(to_json).unwrap_or(J::Null)), true)
            }
            "fail" => {
                max_args(args, 1, "fail")?;
                let reason = match args.first() {
                    Some(Value::Error(e)) => e.message.clone(),
                    Some(v) => v.to_display_string(),
                    None => "failed".into(),
                };
                send(self, Cmd::Fail(reason), true)
            }
            "progress" => {
                max_args(args, 1, "progress")?;
                send(self, Cmd::Progress(args.first().map(to_json).unwrap_or(J::Null)), false)
            }
            "log" => {
                max_args(args, 1, "log")?;
                send(self, Cmd::Log(str_arg(args, 0, "log")?), false)
            }
            other => Err(format!("method '{}' does not exist on BullJob (complete fail progress log)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["complete", "fail", "progress", "log"]
    }
}

// ---------------------------------------------------------------------
// Queue events
// ---------------------------------------------------------------------

fn event_json(e: &QueueEvent, id: &str) -> J {
    let mut m = match e {
        QueueEvent::Added { job_id, name } => json!({ "event": "added", "jobId": job_id, "name": name }),
        QueueEvent::Waiting { job_id, prev } => json!({ "event": "waiting", "jobId": job_id, "prev": prev }),
        QueueEvent::Active { job_id, prev } => json!({ "event": "active", "jobId": job_id, "prev": prev }),
        QueueEvent::Completed { job_id, return_value } => json!({ "event": "completed", "jobId": job_id, "returnvalue": return_value }),
        QueueEvent::Failed { job_id, reason } => json!({ "event": "failed", "jobId": job_id, "failedReason": reason }),
        QueueEvent::Delayed { job_id, delay } => json!({ "event": "delayed", "jobId": job_id, "delay": delay }),
        QueueEvent::Progress { job_id, data } => json!({ "event": "progress", "jobId": job_id, "data": data }),
        QueueEvent::Stalled { job_id } => json!({ "event": "stalled", "jobId": job_id }),
        QueueEvent::Priority { job_id, priority } => json!({ "event": "priority", "jobId": job_id, "priority": priority }),
        QueueEvent::Removed { job_id, prev } => json!({ "event": "removed", "jobId": job_id, "prev": prev }),
        other => json!({ "event": format!("{:?}", other).split([' ', '{', '(']).next().unwrap_or("").to_lowercase() }),
    };
    m["id"] = J::String(id.to_string());
    m
}

fn events(args: &[Value]) -> Result<Value, String> {
    max_args(args, 2, "bullmq.events")?;
    let name = str_arg(args, 0, "bullmq.events")?;
    let o = opts_map(args, 1, "bullmq.events")?;
    let (conn, prefix) = connection(&o)?;
    let mut b = QueueEventsBuilder::new(name.clone()).connection(conn).prefix(prefix);
    if let Some(id) = get_s(&o, "lastEventId") {
        b = b.last_event_id(id);
    }
    let qe = runtime().block_on(b.build()).map_err(err)?;
    let mut rx = qe.subscribe();
    let inbox: Arc<Mutex<VecDeque<J>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ib = Arc::clone(&inbox);
    runtime().spawn(async move {
        let _keep = qe;
        loop {
            match rx.recv().await {
                Ok((e, id)) => ib.lock().unwrap().push_back(event_json(&e, &id)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            }
        }
    });
    Ok(NativeObjectRef::new(EventsValue { inbox, name }).to_value())
}

pub struct EventsValue {
    inbox: Arc<Mutex<VecDeque<J>>>,
    name: String,
}

impl FoguObject for EventsValue {
    fn name(&self) -> &str {
        "BullEvents"
    }

    fn get(&self, field: &str) -> Option<Value> {
        (field == "name").then(|| Value::Str(self.name.clone().into()))
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "tryNext" => Ok(self.inbox.lock().unwrap().pop_front().map_or(Value::Nil, |e| json_to_value(&e))),
            other => Err(format!("method '{}' does not exist on BullEvents (next tryNext)", other)),
        }
    }

    /// `next(timeout?)` → `{ event, jobId, … }`, nil on timeout.
    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        if method != "next" {
            return None;
        }
        let deadline = match args.first() {
            None | Some(Value::Nil) => None,
            Some(v) => match duration_arg(v, "next") {
                Ok(d) => Some(Instant::now() + d),
                Err(e) => return Some(Err(e)),
            },
        };
        let inbox = Arc::clone(&self.inbox);
        Some(Ok(Box::new(move || {
            if let Some(e) = inbox.lock().unwrap().pop_front() {
                return Some(Ok(json_to_value(&e)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Some(Ok(Value::Nil));
            }
            sched::add_timer(Instant::now() + Duration::from_millis(5));
            None
        })))
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["next", "tryNext"]
    }
}
