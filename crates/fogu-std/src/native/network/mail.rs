//! `fogu:mail` module — sending e-mail over SMTP (Gmail, Outlook, Yahoo,
//! iCloud, Amazon SES… or any server) and over the HTTP APIs of Resend,
//! SendGrid, Mailgun, Postmark, Brevo, MailerSend and Mailjet.
//!
//! ```fogu
//! import mail from "fogu:mail"
//!
//! const mailer = mail.connect({ provider: "gmail", user: "me@gmail.com", password: appPassword })?
//! // or { provider: "resend", apiKey }, "smtp://user:pass@host:587", { provider: "console" }…
//! const sent = mailer.send({
//!     from: "Shop <shop@example.com>",
//!     to: ["ana@example.com", { name: "Bob", email: "bob@example.com" }],
//!     subject: "Your order",
//!     text: "Thanks!",
//!     html: "<p>Thanks!</p>",
//!     attachments: [{ filename: "invoice.pdf", path: "out/invoice.pdf" }],
//! })?                                     // { id, provider }
//! ```
//!
//! Sending runs on a background thread and waits cooperatively, so other `fg`
//! tasks keep running. A rejected message, a bad address or a network failure
//! is an `error` value; a malformed configuration stops the program.

use std::cell::RefCell;
use ahash::AHashMap as HashMap;
use std::time::Duration;

use base64::Engine as _;

use crate::runtime::value::{ModuleValue, Value};

use crate::native::object::{FoguObject, NativeObjectRef, Poller};
use crate::native::util::{duration_arg, map, max_args, nfn_var, thread_poller};

pub fn build() -> ModuleValue {
    let mut e: HashMap<String, Value> = HashMap::new();
    e.insert("connect".into(), nfn_var("connect", 1, connect));
    e.insert(
        "providers".into(),
        crate::native::util::str_list(PROVIDERS.iter().map(|s| s.to_string())),
    );
    ModuleValue {
        name: "fogu:mail".into(),
        exports: RefCell::new(e),
    }
}

const PROVIDERS: &[&str] = &[
    "smtp", "gmail", "outlook", "office365", "yahoo", "icloud", "zoho", "fastmail", "aol", "gmx", "ses",
    "mailtrap", "mailpit", "resend", "sendgrid", "mailgun", "postmark", "brevo", "mailersend", "mailjet",
    "console",
];

// ---------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Security {
    /// TLS from the first byte (port 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (ports 587, 25, 2525).
    StartTls,
    /// No encryption (local test servers only).
    None,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Api {
    Resend,
    SendGrid,
    Mailgun,
    Postmark,
    Brevo,
    MailerSend,
    Mailjet,
}

impl Api {
    fn name(self) -> &'static str {
        match self {
            Api::Resend => "resend",
            Api::SendGrid => "sendgrid",
            Api::Mailgun => "mailgun",
            Api::Postmark => "postmark",
            Api::Brevo => "brevo",
            Api::MailerSend => "mailersend",
            Api::Mailjet => "mailjet",
        }
    }
    fn base(self) -> &'static str {
        match self {
            Api::Resend => "https://api.resend.com",
            Api::SendGrid => "https://api.sendgrid.com",
            Api::Mailgun => "https://api.mailgun.net",
            Api::Postmark => "https://api.postmarkapp.com",
            Api::Brevo => "https://api.brevo.com",
            Api::MailerSend => "https://api.mailersend.com",
            Api::Mailjet => "https://api.mailjet.com",
        }
    }
    /// Environment variable read when `apiKey` is not given.
    fn key_env(self) -> &'static str {
        match self {
            Api::Resend => "RESEND_API_KEY",
            Api::SendGrid => "SENDGRID_API_KEY",
            Api::Mailgun => "MAILGUN_API_KEY",
            Api::Postmark => "POSTMARK_SERVER_TOKEN",
            Api::Brevo => "BREVO_API_KEY",
            Api::MailerSend => "MAILERSEND_API_KEY",
            Api::Mailjet => "MAILJET_API_KEY",
        }
    }
}

#[derive(Clone, Debug)]
enum Transport {
    Smtp {
        name: String,
        host: String,
        port: u16,
        security: Security,
        user: Option<String>,
        password: Option<String>,
    },
    Api {
        api: Api,
        key: String,
        /// Mailjet's secret key.
        secret: Option<String>,
        base: String,
        /// Mailgun's sending domain.
        domain: Option<String>,
    },
    Console,
}

impl Transport {
    fn name(&self) -> String {
        match self {
            Transport::Smtp { name, .. } => name.clone(),
            Transport::Api { api, .. } => api.name().into(),
            Transport::Console => "console".into(),
        }
    }
}

#[derive(Clone)]
struct Config {
    transport: Transport,
    from: Option<Addr>,
    timeout: Duration,
}

/// SMTP presets: host, port, security.
fn smtp_preset(name: &str, region: Option<&str>) -> Option<(String, u16, Security)> {
    use Security::*;
    let (host, port, sec) = match name {
        "gmail" | "google" => ("smtp.gmail.com".to_string(), 587, StartTls),
        "outlook" | "hotmail" | "live" => ("smtp-mail.outlook.com".into(), 587, StartTls),
        "office365" | "microsoft365" | "m365" => ("smtp.office365.com".into(), 587, StartTls),
        "yahoo" => ("smtp.mail.yahoo.com".into(), 465, Tls),
        "icloud" => ("smtp.mail.me.com".into(), 587, StartTls),
        "zoho" => (
            if region == Some("eu") { "smtp.zoho.eu" } else { "smtp.zoho.com" }.into(),
            465,
            Tls,
        ),
        "fastmail" => ("smtp.fastmail.com".into(), 465, Tls),
        "aol" => ("smtp.aol.com".into(), 465, Tls),
        "gmx" => ("mail.gmx.com".into(), 587, StartTls),
        "ses" | "amazon-ses" => (format!("email-smtp.{}.amazonaws.com", region.unwrap_or("us-east-1")), 587, StartTls),
        "mailtrap" => ("sandbox.smtp.mailtrap.io".into(), 2525, StartTls),
        "mailpit" | "mailhog" | "local" => ("127.0.0.1".into(), 1025, None),
        _ => return Option::None,
    };
    Some((host, port, sec))
}

fn api_of(name: &str) -> Option<Api> {
    Some(match name {
        "resend" => Api::Resend,
        "sendgrid" => Api::SendGrid,
        "mailgun" => Api::Mailgun,
        "postmark" => Api::Postmark,
        "brevo" | "sendinblue" => Api::Brevo,
        "mailersend" => Api::MailerSend,
        "mailjet" => Api::Mailjet,
        _ => return None,
    })
}

fn get_str(m: &HashMap<String, Value>, k: &str) -> Option<String> {
    match m.get(k) {
        Some(Value::Str(s)) if !s.is_empty() => Some(s.to_string()),
        Some(Value::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// `smtp://user:pass@host:587`, `smtps://…:465`, `?tls=none`; `console://`.
fn config_from_url(url: &str) -> Result<HashMap<String, Value>, String> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| format!("mail.connect: invalid url '{}'", url))?;
    let mut m: HashMap<String, Value> = HashMap::new();
    let s = |v: &str| Value::Str(v.to_string().into());
    match scheme {
        "console" => {
            m.insert("provider".into(), s("console"));
            return Ok(m);
        }
        "smtp" | "smtps" => {}
        other => return Err(format!("mail.connect: unknown url scheme '{}' (smtp, smtps, console)", other)),
    }
    let (main, query) = rest.split_once('?').unwrap_or((rest, ""));
    let main = main.trim_end_matches('/');
    let (auth, host_port) = match main.rfind('@') {
        Some(i) => (Some(&main[..i]), &main[i + 1..]),
        None => (None, main),
    };
    if let Some(auth) = auth {
        let (u, p) = auth.split_once(':').unwrap_or((auth, ""));
        m.insert("user".into(), s(&crate::native::util::url_decode(u)));
        if !p.is_empty() {
            m.insert("password".into(), s(&crate::native::util::url_decode(p)));
        }
    }
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_port, None),
    };
    m.insert("provider".into(), s("smtp"));
    m.insert("host".into(), s(host));
    if let Some(p) = port {
        let n: i64 = p.parse().map_err(|_| format!("mail.connect: invalid port '{}'", p))?;
        m.insert("port".into(), Value::Int(n));
    }
    m.insert("secure".into(), s(if scheme == "smtps" { "tls" } else { "auto" }));
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        if let Some(("tls", v)) = pair.split_once('=') {
            m.insert("secure".into(), s(v));
        }
    }
    Ok(m)
}

fn parse_config(v: &Value) -> Result<Config, String> {
    let m = match v {
        Value::Str(url) => config_from_url(url)?,
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("mail.connect expects a map or a url, got '{}'", other.type_name())),
    };
    let provider = get_str(&m, "provider").unwrap_or_else(|| "smtp".into()).to_lowercase();
    let region = get_str(&m, "region");
    let timeout = match m.get("timeout") {
        None | Some(Value::Nil) => Duration::from_secs(30),
        Some(t) => duration_arg(t, "mail.connect: timeout")?,
    };
    let from = match m.get("from") {
        None | Some(Value::Nil) => None,
        Some(f) => Some(parse_addr(f, "from").map_err(strip_err)?),
    };

    let transport = if provider == "console" {
        Transport::Console
    } else if let Some(api) = api_of(&provider) {
        let key = get_str(&m, "apiKey")
            .or_else(|| get_str(&m, "serverToken"))
            .or_else(|| get_str(&m, "token"))
            .or_else(|| env(api.key_env()))
            .ok_or_else(|| format!("mail.connect: {} needs 'apiKey' (or ${})", api.name(), api.key_env()))?;
        let secret = match api {
            Api::Mailjet => Some(
                get_str(&m, "apiSecret")
                    .or_else(|| env("MAILJET_SECRET_KEY"))
                    .ok_or("mail.connect: mailjet needs 'apiSecret' (or $MAILJET_SECRET_KEY)")?,
            ),
            _ => None,
        };
        let domain = match api {
            Api::Mailgun => Some(
                get_str(&m, "domain")
                    .or_else(|| env("MAILGUN_DOMAIN"))
                    .ok_or("mail.connect: mailgun needs 'domain' (or $MAILGUN_DOMAIN)")?,
            ),
            _ => None,
        };
        let base = match get_str(&m, "baseUrl") {
            Some(b) => b,
            None if api == Api::Mailgun && region.as_deref() == Some("eu") => "https://api.eu.mailgun.net".into(),
            None => api.base().into(),
        };
        Transport::Api { api, key, secret, base: base.trim_end_matches('/').into(), domain }
    } else {
        let preset = smtp_preset(&provider, region.as_deref());
        if preset.is_none() && provider != "smtp" {
            return Err(format!("mail.connect: unknown provider '{}' ({})", provider, PROVIDERS.join(", ")));
        }
        let host = get_str(&m, "host")
            .or_else(|| preset.as_ref().map(|p| p.0.clone()))
            .ok_or("mail.connect: smtp needs 'host'")?;
        let port = match m.get("port") {
            Some(Value::Int(p)) if (1..=65535).contains(p) => Some(*p as u16),
            None | Some(Value::Nil) => None,
            Some(other) => return Err(format!("mail.connect: invalid port {}", other.to_display_string())),
        };
        let security = match get_str(&m, "secure").as_deref() {
            Some("tls") | Some("ssl") | Some("true") => Some(Security::Tls),
            Some("starttls") => Some(Security::StartTls),
            Some("none") | Some("false") | Some("plain") => Some(Security::None),
            Some("auto") | None => match m.get("secure") {
                Some(Value::Bool(true)) => Some(Security::Tls),
                Some(Value::Bool(false)) => Some(Security::StartTls),
                _ => None,
            },
            Some(other) => return Err(format!("mail.connect: secure must be \"tls\", \"starttls\" or \"none\", got '{}'", other)),
        };
        let (port, security) = match (port, security, &preset) {
            (Some(p), Some(s), _) => (p, s),
            (Some(p), None, _) => (p, if p == 465 { Security::Tls } else { Security::StartTls }),
            (None, Some(s), _) => (if s == Security::Tls { 465 } else { 587 }, s),
            (None, None, Some((_, p, s))) => (*p, *s),
            (None, None, None) => (587, Security::StartTls),
        };
        Transport::Smtp {
            name: if provider == "smtp" { "smtp".into() } else { provider.clone() },
            host,
            port,
            security,
            user: get_str(&m, "user").or_else(|| get_str(&m, "username")),
            password: get_str(&m, "password").or_else(|| get_str(&m, "pass")),
        }
    };
    Ok(Config { transport, from, timeout })
}

fn strip_err(e: String) -> String {
    e.strip_prefix(crate::runtime::value::ERR_VALUE_PREFIX).map(|s| s.to_string()).unwrap_or(e)
}

// ---------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Addr {
    name: Option<String>,
    email: String,
}

impl Addr {
    /// `Name <email>` (quoted when needed) or just `email`.
    fn display(&self) -> String {
        match &self.name {
            Some(n) if n.chars().any(|c| ",;:<>@\"()[]".contains(c)) => {
                format!("\"{}\" <{}>", n.replace('\\', "\\\\").replace('"', "\\\""), self.email)
            }
            Some(n) => format!("{} <{}>", n, self.email),
            None => self.email.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct Attach {
    filename: String,
    content: Vec<u8>,
    content_type: String,
    /// Content-ID of an inline attachment (`<img src="cid:logo">`).
    cid: Option<String>,
}

#[derive(Clone, Debug)]
struct Mail {
    from: Addr,
    to: Vec<Addr>,
    cc: Vec<Addr>,
    bcc: Vec<Addr>,
    reply_to: Vec<Addr>,
    subject: String,
    text: Option<String>,
    html: Option<String>,
    attachments: Vec<Attach>,
    headers: Vec<(String, String)>,
}

/// `"a@x.com"`, `"Ana <a@x.com>"` or `{ name, email }`. A bad address is an
/// operational error (it usually comes from user data).
fn parse_addr(v: &Value, what: &str) -> Result<Addr, String> {
    let (name, email) = match v {
        Value::Str(s) => {
            let s = s.trim();
            match (s.rfind('<'), s.ends_with('>')) {
                (Some(i), true) => {
                    let name = s[..i].trim().trim_matches('"').trim();
                    (if name.is_empty() { None } else { Some(name.to_string()) }, s[i + 1..s.len() - 1].trim().to_string())
                }
                _ => (None, s.to_string()),
            }
        }
        Value::Map(m) => {
            let m = m.borrow();
            let email = match m.get("email").or_else(|| m.get("address")) {
                Some(Value::Str(e)) => e.trim().to_string(),
                _ => return Err(format!("mail: '{}' needs an 'email'", what)),
            };
            (get_str(&m, "name"), email)
        }
        other => return Err(format!("mail: '{}' must be a str or a {{ name, email }} map, got '{}'", what, other.type_name())),
    };
    if !valid_email(&email) {
        return Err(io_err!("mail: invalid address '{}' in '{}'", email, what));
    }
    Ok(Addr { name, email })
}

fn parse_addrs(v: Option<&Value>, what: &str) -> Result<Vec<Addr>, String> {
    match v {
        None | Some(Value::Nil) => Ok(Vec::new()),
        Some(Value::List(l)) => l.borrow().iter().map(|a| parse_addr(a, what)).collect(),
        Some(Value::Str(s)) if s.contains(',') && !s.contains('"') => {
            s.split(',').map(|a| parse_addr(&Value::Str(a.trim().to_string().into()), what)).collect()
        }
        Some(one) => Ok(vec![parse_addr(one, what)?]),
    }
}

fn guess_type(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "ics" => "text/calendar",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

fn parse_attachment(v: &Value) -> Result<Attach, String> {
    let m = match v {
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("mail: an attachment must be a map, got '{}'", other.type_name())),
    };
    let path = get_str(&m, "path");
    let content = match (m.get("content"), &path) {
        (Some(Value::Str(s)), _) => match get_str(&m, "encoding").as_deref() {
            Some("base64") => base64::engine::general_purpose::STANDARD
                .decode(s.trim())
                .map_err(|_| "mail: attachment content is not valid base64".to_string())?,
            _ => s.as_bytes().to_vec(),
        },
        (Some(Value::List(l)), _) => l
            .borrow()
            .iter()
            .map(|b| match b {
                Value::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err("mail: attachment bytes must be ints 0..255".to_string()),
            })
            .collect::<Result<Vec<u8>, String>>()?,
        (_, Some(p)) => std::fs::read(p).map_err(|e| io_err!("mail: cannot read attachment {}: {}", p, e))?,
        _ => return Err("mail: an attachment needs 'content' or 'path'".into()),
    };
    let filename = get_str(&m, "filename")
        .or_else(|| path.as_ref().map(|p| p.replace('\\', "/").rsplit('/').next().unwrap_or(p).to_string()))
        .ok_or("mail: an attachment needs a 'filename'")?;
    let content_type = get_str(&m, "contentType").unwrap_or_else(|| guess_type(&filename).into());
    Ok(Attach { filename, content, content_type, cid: get_str(&m, "cid") })
}

fn parse_mail(v: &Value, default_from: &Option<Addr>) -> Result<Mail, String> {
    let m = match v {
        Value::Map(m) => m.borrow().clone(),
        other => return Err(format!("send expects a message map, got '{}'", other.type_name())),
    };
    let from = match m.get("from") {
        None | Some(Value::Nil) => default_from.clone().ok_or("send: the message needs 'from' (or set it in connect)")?,
        Some(f) => parse_addr(f, "from")?,
    };
    let to = parse_addrs(m.get("to"), "to")?;
    let cc = parse_addrs(m.get("cc"), "cc")?;
    let bcc = parse_addrs(m.get("bcc"), "bcc")?;
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err("send: the message needs at least one recipient in 'to', 'cc' or 'bcc'".into());
    }
    let text = get_str(&m, "text");
    let html = get_str(&m, "html");
    if text.is_none() && html.is_none() {
        return Err("send: the message needs 'text' or 'html'".into());
    }
    let attachments = match m.get("attachments") {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::List(l)) => l.borrow().iter().map(parse_attachment).collect::<Result<_, _>>()?,
        Some(one) => vec![parse_attachment(one)?],
    };
    let mut headers: Vec<(String, String)> = match m.get("headers") {
        Some(Value::Map(h)) => h.borrow().iter().map(|(k, v)| (k.clone(), v.to_display_string())).collect(),
        None | Some(Value::Nil) => Vec::new(),
        Some(other) => return Err(format!("send: headers must be a map, got '{}'", other.type_name())),
    };
    headers.sort();
    for (k, _) in &headers {
        if k.is_empty() || !k.is_ascii() || k.contains([':', ' ']) {
            return Err(format!("send: invalid header name '{}'", k));
        }
    }
    Ok(Mail {
        from,
        to,
        cc,
        bcc,
        reply_to: parse_addrs(m.get("replyTo"), "replyTo")?,
        subject: get_str(&m, "subject").unwrap_or_default(),
        text,
        html,
        attachments,
        headers,
    })
}

// ---------------------------------------------------------------------
// SMTP and MIME (our own: RFC 5321, 5322, 2045-2047, 3207, 4954)
// ---------------------------------------------------------------------

/// `local@domain` with a sane local part and domain (no IDN or quoted forms).
fn valid_email(e: &str) -> bool {
    let Some((local, domain)) = e.rsplit_once('@') else { return false };
    let ok_local = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c) || !c.is_ascii());
    let labels: Vec<&str> = domain.split('.').collect();
    let ok_domain = domain.len() <= 253
        && (labels.len() >= 2 || domain.starts_with('['))
        && labels.iter().all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '[' || c == ']' || c == ':' || !c.is_ascii()));
    ok_local && ok_domain
}

/// RFC 2047 encoded word when the text is not plain ASCII.
fn encode_word(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        s.to_string()
    } else {
        format!("=?utf-8?b?{}?=", b64(s.as_bytes()))
    }
}

fn mailbox_header(a: &Addr) -> String {
    match &a.name {
        None => format!("<{}>", a.email),
        Some(n) if n.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) => {
            format!("\"{}\" <{}>", n.replace('\\', "\\\\").replace('"', "\\\""), a.email)
        }
        Some(n) => format!("{} <{}>", encode_word(n), a.email),
    }
}

/// RFC 2822 date of now (UTC).
fn rfc2822_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (h, mi, s) = (secs.rem_euclid(86_400) / 3600, secs.rem_euclid(3600) / 60, secs.rem_euclid(60));
    // civil date (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} +0000", DAYS[days.rem_euclid(7) as usize], d, MONTHS[(m - 1) as usize], y, h, mi, s)
}

/// Quoted-printable (RFC 2045), CRLF line endings.
fn quoted_printable(text: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.replace("\r\n", "\n").split('\n').enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        let mut col = 0;
        let bytes = line.as_bytes();
        for (j, &b) in bytes.iter().enumerate() {
            let last = j + 1 == bytes.len();
            let enc = if (b == b' ' || b == b'\t') && last {
                format!("={:02X}", b)
            } else if b == b'=' || b > 126 || (b < 32 && b != b'\t') {
                format!("={:02X}", b)
            } else {
                (b as char).to_string()
            };
            if col + enc.len() > 75 {
                out.push_str("=\r\n");
                col = 0;
            }
            out.push_str(&enc);
            col += enc.len();
        }
    }
    out
}

fn base64_lines(data: &[u8]) -> String {
    let s = b64(data);
    s.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join("\r\n")
}

fn boundary() -> String {
    format!("=_fogu_{}", uuid::Uuid::new_v4().simple())
}

fn text_part(content_type: &str, body: &str) -> String {
    let ascii = body.is_ascii() && body.lines().all(|l| l.len() < 900);
    if ascii {
        format!("Content-Type: {}; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{}", content_type, body.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        format!("Content-Type: {}; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{}", content_type, quoted_printable(body))
    }
}

fn multipart(kind: &str, parts: &[String]) -> String {
    let b = boundary();
    let mut s = format!("Content-Type: multipart/{}; boundary=\"{}\"\r\n\r\n", kind, b);
    for p in parts {
        s.push_str(&format!("--{}\r\n{}\r\n", b, p));
    }
    s.push_str(&format!("--{}--\r\n", b));
    s
}

/// The RFC 5322 message: headers and MIME body (Bcc is not a header).
struct Built {
    id: String,
    text: String,
}

/// The message (also used by `render`).
fn build_message(mail: &Mail) -> Result<Built, String> {
    let host = mail.from.email.rsplit_once('@').map(|(_, d)| d.to_string()).unwrap_or_else(|| "localhost".into());
    let id = format!("{}@{}", uuid::Uuid::new_v4().simple(), host);
    let mut h = vec![
        format!("Date: {}", rfc2822_now()),
        format!("From: {}", mailbox_header(&mail.from)),
    ];
    let list = |v: &[Addr]| v.iter().map(mailbox_header).collect::<Vec<_>>().join(", ");
    if !mail.to.is_empty() {
        h.push(format!("To: {}", list(&mail.to)));
    }
    if !mail.cc.is_empty() {
        h.push(format!("Cc: {}", list(&mail.cc)));
    }
    if !mail.reply_to.is_empty() {
        h.push(format!("Reply-To: {}", list(&mail.reply_to)));
    }
    h.push(format!("Subject: {}", encode_word(&mail.subject)));
    h.push(format!("Message-ID: <{}>", id));
    h.push("MIME-Version: 1.0".into());
    for (k, v) in &mail.headers {
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_graphic() && c != ':') {
            return Err(format!("send: invalid header name '{}'", k));
        }
        if v.contains(['\r', '\n']) {
            return Err(format!("send: the value of header '{}' has a line break", k));
        }
        h.push(format!("{}: {}", k, encode_word(v)));
    }
    let body = match (&mail.text, &mail.html) {
        (Some(t), Some(html)) => multipart("alternative", &[text_part("text/plain", t), text_part("text/html", html)]),
        (Some(t), None) => text_part("text/plain", t),
        (None, Some(html)) => text_part("text/html", html),
        (None, None) => text_part("text/plain", ""),
    };
    let body = if mail.attachments.is_empty() {
        body
    } else {
        let mut parts = vec![body];
        for a in &mail.attachments {
            if a.content_type.contains(['\r', '\n']) || !a.content_type.contains('/') {
                return Err(format!("mail: invalid content type '{}'", a.content_type));
            }
            let name = encode_word(&a.filename.replace('"', ""));
            let disposition = match &a.cid {
                Some(cid) => format!("Content-Disposition: inline; filename=\"{}\"\r\nContent-ID: <{}>", name, cid),
                None => format!("Content-Disposition: attachment; filename=\"{}\"", name),
            };
            parts.push(format!(
                "Content-Type: {}; name=\"{}\"\r\n{}\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
                a.content_type,
                name,
                disposition,
                base64_lines(&a.content)
            ));
        }
        multipart("mixed", &parts)
    };
    Ok(Built { id, text: format!("{}\r\n{}", h.join("\r\n"), body) })
}

enum Wire {
    Plain(std::net::TcpStream),
    Tls(Box<native_tls::TlsStream<std::net::TcpStream>>),
}

impl std::io::Read for Wire {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.read(b),
            Wire::Tls(s) => s.read(b),
        }
    }
}

impl std::io::Write for Wire {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(s) => s.write(b),
            Wire::Tls(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Wire::Plain(s) => s.flush(),
            Wire::Tls(s) => s.flush(),
        }
    }
}

struct Smtp {
    wire: Option<Wire>,
    host: String,
    ext: Vec<String>,
}

impl Smtp {
    /// One reply: (code, text of every line).
    fn reply(&mut self) -> Result<(u16, String), String> {
        use std::io::Read;
        let mut text = String::new();
        loop {
            let mut line = Vec::new();
            let mut b = [0u8; 1];
            loop {
                let n = self.wire.as_mut().ok_or("not connected")?.read(&mut b).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("the server closed the connection".into());
                }
                line.push(b[0]);
                if b[0] == b'\n' || line.len() > 4096 {
                    break;
                }
            }
            let l = String::from_utf8_lossy(&line).trim_end().to_string();
            let code: u16 = l.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| format!("unexpected reply '{}'", l))?;
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(l.get(4..).unwrap_or(""));
            if l.as_bytes().get(3) != Some(&b'-') {
                return Ok((code, text));
            }
        }
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        use std::io::Write;
        let w = self.wire.as_mut().ok_or("not connected")?;
        w.write_all(line.as_bytes()).and_then(|_| w.write_all(b"\r\n")).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }

    /// Sends a command and expects a reply class (2 = 2xx, 3 = 3xx).
    fn cmd(&mut self, line: &str, class: u16, shown: &str) -> Result<String, String> {
        self.send(line)?;
        let (code, text) = self.reply()?;
        if code / 100 != class {
            return Err(format!("{} → {} {}", shown, code, text.replace('\n', " ")));
        }
        Ok(text)
    }

    fn ehlo(&mut self) -> Result<(), String> {
        let me = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).ok().filter(|h| !h.is_empty() && h.is_ascii()).unwrap_or_else(|| "localhost".into());
        let text = match self.cmd(&format!("EHLO {}", me), 2, "EHLO") {
            Ok(t) => t,
            Err(_) => self.cmd(&format!("HELO {}", me), 2, "HELO")?,
        };
        self.ext = text.lines().skip(1).map(|l| l.trim().to_ascii_uppercase()).collect();
        Ok(())
    }

    fn has(&self, ext: &str) -> bool {
        self.ext.iter().any(|e| e == ext || e.starts_with(&format!("{} ", ext)))
    }

    fn auth_mechs(&self) -> Vec<String> {
        self.ext.iter().filter_map(|e| e.strip_prefix("AUTH ").or_else(|| e.strip_prefix("AUTH="))).flat_map(|m| m.split_whitespace().map(String::from)).collect()
    }

    fn tls(&mut self, tcp: std::net::TcpStream) -> Result<Wire, String> {
        let c = native_tls::TlsConnector::new().map_err(|e| format!("TLS: {}", e))?;
        Ok(Wire::Tls(Box::new(c.connect(&self.host, tcp).map_err(|e| format!("TLS handshake with {}: {}", self.host, e))?)))
    }

    /// Connects, says hello, upgrades to TLS and logs in.
    fn open(t: &Transport, timeout: Duration) -> Result<Smtp, String> {
        let Transport::Smtp { host, port, security, user, password, .. } = t else { unreachable!() };
        use std::net::ToSocketAddrs;
        let addr = (host.as_str(), *port).to_socket_addrs().map_err(|e| format!("{}: {}", host, e))?.next().ok_or_else(|| format!("{}: no address", host))?;
        let tcp = std::net::TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("cannot connect to {}:{}: {}", host, port, e))?;
        tcp.set_read_timeout(Some(timeout)).ok();
        tcp.set_write_timeout(Some(timeout)).ok();
        let mut s = Smtp { wire: None, host: host.clone(), ext: Vec::new() };
        s.wire = Some(match security {
            Security::Tls => s.tls(tcp)?,
            _ => Wire::Plain(tcp),
        });
        let (code, text) = s.reply()?;
        if code != 220 {
            return Err(format!("greeting → {} {}", code, text));
        }
        s.ehlo()?;
        if matches!(security, Security::StartTls) {
            if !s.has("STARTTLS") {
                return Err(format!("{} does not offer STARTTLS", host));
            }
            s.cmd("STARTTLS", 2, "STARTTLS")?;
            let Some(Wire::Plain(tcp)) = s.wire.take() else { return Err("STARTTLS on a TLS connection".into()) };
            s.wire = Some(s.tls(tcp)?);
            s.ehlo()?;
        }
        if let Some(u) = user {
            let pw = password.clone().unwrap_or_default();
            let mechs = s.auth_mechs();
            if mechs.iter().any(|m| m == "PLAIN") || mechs.is_empty() {
                s.cmd(&format!("AUTH PLAIN {}", b64(format!("\0{}\0{}", u, pw).as_bytes())), 2, "AUTH PLAIN")?;
            } else if mechs.iter().any(|m| m == "LOGIN") {
                s.cmd("AUTH LOGIN", 3, "AUTH LOGIN")?;
                s.cmd(&b64(u.as_bytes()), 3, "AUTH LOGIN (user)")?;
                s.cmd(&b64(pw.as_bytes()), 2, "AUTH LOGIN (password)")?;
            } else {
                return Err(format!("no supported login method (server offers {})", mechs.join(" ")));
            }
        }
        Ok(s)
    }

    fn quit(&mut self) {
        let _ = self.send("QUIT");
        let _ = self.reply();
    }
}

fn smtp_send(t: &Transport, timeout: Duration, mail: &Mail) -> Result<String, String> {
    let message = build_message(mail)?;
    let run = || -> Result<(), String> {
        let mut s = Smtp::open(t, timeout)?;
        let utf8 = !mail.from.email.is_ascii() || mail.to.iter().chain(&mail.cc).chain(&mail.bcc).any(|a| !a.email.is_ascii());
        let extra = if utf8 && s.has("SMTPUTF8") { " SMTPUTF8" } else { "" };
        s.cmd(&format!("MAIL FROM:<{}>{}", mail.from.email, extra), 2, "MAIL FROM")?;
        for a in mail.to.iter().chain(&mail.cc).chain(&mail.bcc) {
            s.cmd(&format!("RCPT TO:<{}>", a.email), 2, &format!("RCPT TO <{}>", a.email))?;
        }
        s.cmd("DATA", 3, "DATA")?;
        // dot-stuffing: a line starting with "." gets another one
        let mut data = String::with_capacity(message.text.len() + 64);
        for line in message.text.split("\r\n") {
            if line.starts_with('.') {
                data.push('.');
            }
            data.push_str(line);
            data.push_str("\r\n");
        }
        data.push('.');
        s.cmd(&data, 2, "message")?;
        s.quit();
        Ok(())
    };
    run().map_err(|e| io_err!("mail: {} smtp: {}", t.name(), e))?;
    Ok(message.id)
}

fn smtp_verify(t: &Transport, timeout: Duration) -> Result<bool, String> {
    let mut s = Smtp::open(t, timeout).map_err(|e| io_err!("mail: {} smtp: {}", t.name(), e))?;
    s.quit();
    Ok(true)
}

// ---------------------------------------------------------------------
// HTTP APIs
// ---------------------------------------------------------------------

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn addr_obj(a: &Addr, email_key: &str, name_key: &str) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert(email_key.into(), a.email.clone().into());
    if let Some(n) = &a.name {
        m.insert(name_key.into(), n.clone().into());
    }
    serde_json::Value::Object(m)
}

fn addr_list(v: &[Addr], email_key: &str, name_key: &str) -> serde_json::Value {
    v.iter().map(|a| addr_obj(a, email_key, name_key)).collect()
}

fn joined(v: &[Addr]) -> String {
    v.iter().map(Addr::display).collect::<Vec<_>>().join(", ")
}

fn headers_obj(h: &[(String, String)]) -> serde_json::Value {
    serde_json::Value::Object(h.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect())
}

/// Request body (JSON) of each provider; Mailgun uses a multipart form.
fn api_payload(api: Api, mail: &Mail) -> serde_json::Value {
    use serde_json::json;
    let mut body = serde_json::Map::new();
    let mut put = |k: &str, v: serde_json::Value| {
        let empty = v.is_null() || v.as_array().is_some_and(|a| a.is_empty()) || v.as_object().is_some_and(|o| o.is_empty());
        if !empty {
            body.insert(k.into(), v);
        }
    };
    let opt = |s: &Option<String>| s.clone().map_or(serde_json::Value::Null, Into::into);
    match api {
        Api::Resend => {
            put("from", mail.from.display().into());
            put("to", mail.to.iter().map(Addr::display).collect());
            put("cc", mail.cc.iter().map(Addr::display).collect());
            put("bcc", mail.bcc.iter().map(Addr::display).collect());
            put("reply_to", mail.reply_to.iter().map(Addr::display).collect());
            put("subject", mail.subject.clone().into());
            put("text", opt(&mail.text));
            put("html", opt(&mail.html));
            put("headers", headers_obj(&mail.headers));
            put(
                "attachments",
                mail.attachments
                    .iter()
                    .map(|a| {
                        let mut o = json!({ "filename": a.filename, "content": b64(&a.content), "content_type": a.content_type });
                        if let Some(cid) = &a.cid {
                            o["content_id"] = cid.clone().into();
                        }
                        o
                    })
                    .collect(),
            );
        }
        Api::SendGrid => {
            let mut p = serde_json::Map::new();
            p.insert("to".into(), addr_list(&mail.to, "email", "name"));
            if !mail.cc.is_empty() {
                p.insert("cc".into(), addr_list(&mail.cc, "email", "name"));
            }
            if !mail.bcc.is_empty() {
                p.insert("bcc".into(), addr_list(&mail.bcc, "email", "name"));
            }
            put("personalizations", json!([p]));
            put("from", addr_obj(&mail.from, "email", "name"));
            match mail.reply_to.len() {
                0 => {}
                1 => put("reply_to", addr_obj(&mail.reply_to[0], "email", "name")),
                _ => put("reply_to_list", addr_list(&mail.reply_to, "email", "name")),
            }
            put("subject", mail.subject.clone().into());
            let mut content = Vec::new();
            if let Some(t) = &mail.text {
                content.push(json!({ "type": "text/plain", "value": t }));
            }
            if let Some(h) = &mail.html {
                content.push(json!({ "type": "text/html", "value": h }));
            }
            put("content", content.into());
            put("headers", headers_obj(&mail.headers));
            put(
                "attachments",
                mail.attachments
                    .iter()
                    .map(|a| {
                        let mut o = json!({
                            "content": b64(&a.content), "filename": a.filename, "type": a.content_type,
                            "disposition": if a.cid.is_some() { "inline" } else { "attachment" },
                        });
                        if let Some(cid) = &a.cid {
                            o["content_id"] = cid.clone().into();
                        }
                        o
                    })
                    .collect(),
            );
        }
        Api::Postmark => {
            put("From", mail.from.display().into());
            put("To", joined(&mail.to).into());
            put("Cc", joined(&mail.cc).into());
            put("Bcc", joined(&mail.bcc).into());
            put("ReplyTo", joined(&mail.reply_to).into());
            put("Subject", mail.subject.clone().into());
            put("TextBody", opt(&mail.text));
            put("HtmlBody", opt(&mail.html));
            put("Headers", mail.headers.iter().map(|(k, v)| json!({ "Name": k, "Value": v })).collect());
            put(
                "Attachments",
                mail.attachments
                    .iter()
                    .map(|a| {
                        let mut o = json!({ "Name": a.filename, "Content": b64(&a.content), "ContentType": a.content_type });
                        if let Some(cid) = &a.cid {
                            o["ContentID"] = format!("cid:{}", cid).into();
                        }
                        o
                    })
                    .collect(),
            );
            body.retain(|k, v| !(k != "To" && v.as_str() == Some("")));
        }
        Api::Brevo => {
            put("sender", addr_obj(&mail.from, "email", "name"));
            put("to", addr_list(&mail.to, "email", "name"));
            put("cc", addr_list(&mail.cc, "email", "name"));
            put("bcc", addr_list(&mail.bcc, "email", "name"));
            if let Some(r) = mail.reply_to.first() {
                put("replyTo", addr_obj(r, "email", "name"));
            }
            put("subject", mail.subject.clone().into());
            put("textContent", opt(&mail.text));
            put("htmlContent", opt(&mail.html));
            put("headers", headers_obj(&mail.headers));
            put(
                "attachment",
                mail.attachments.iter().map(|a| json!({ "name": a.filename, "content": b64(&a.content) })).collect(),
            );
        }
        Api::MailerSend => {
            put("from", addr_obj(&mail.from, "email", "name"));
            put("to", addr_list(&mail.to, "email", "name"));
            put("cc", addr_list(&mail.cc, "email", "name"));
            put("bcc", addr_list(&mail.bcc, "email", "name"));
            if let Some(r) = mail.reply_to.first() {
                put("reply_to", addr_obj(r, "email", "name"));
            }
            put("subject", mail.subject.clone().into());
            put("text", opt(&mail.text));
            put("html", opt(&mail.html));
            put(
                "attachments",
                mail.attachments
                    .iter()
                    .map(|a| {
                        let mut o = json!({ "filename": a.filename, "content": b64(&a.content),
                            "disposition": if a.cid.is_some() { "inline" } else { "attachment" } });
                        if let Some(cid) = &a.cid {
                            o["id"] = cid.clone().into();
                        }
                        o
                    })
                    .collect(),
            );
        }
        Api::Mailjet => {
            let mut m = serde_json::Map::new();
            m.insert("From".into(), addr_obj(&mail.from, "Email", "Name"));
            m.insert("To".into(), addr_list(&mail.to, "Email", "Name"));
            if !mail.cc.is_empty() {
                m.insert("Cc".into(), addr_list(&mail.cc, "Email", "Name"));
            }
            if !mail.bcc.is_empty() {
                m.insert("Bcc".into(), addr_list(&mail.bcc, "Email", "Name"));
            }
            if let Some(r) = mail.reply_to.first() {
                m.insert("ReplyTo".into(), addr_obj(r, "Email", "Name"));
            }
            m.insert("Subject".into(), mail.subject.clone().into());
            if let Some(t) = &mail.text {
                m.insert("TextPart".into(), t.clone().into());
            }
            if let Some(h) = &mail.html {
                m.insert("HTMLPart".into(), h.clone().into());
            }
            if !mail.headers.is_empty() {
                m.insert("Headers".into(), headers_obj(&mail.headers));
            }
            let (inline, regular): (Vec<&Attach>, Vec<&Attach>) = mail.attachments.iter().partition(|a| a.cid.is_some());
            let att = |list: Vec<&Attach>| -> serde_json::Value {
                list.iter()
                    .map(|a| {
                        let mut o = json!({ "ContentType": a.content_type, "Filename": a.filename, "Base64Content": b64(&a.content) });
                        if let Some(cid) = &a.cid {
                            o["ContentID"] = cid.clone().into();
                        }
                        o
                    })
                    .collect()
            };
            if !regular.is_empty() {
                m.insert("Attachments".into(), att(regular));
            }
            if !inline.is_empty() {
                m.insert("InlinedAttachments".into(), att(inline));
            }
            put("Messages", json!([m]));
        }
        Api::Mailgun => {}
    }
    serde_json::Value::Object(body)
}

/// The message id from a provider's answer.
fn api_message_id(api: Api, headers: &reqwest::header::HeaderMap, body: &serde_json::Value) -> String {
    let s = |v: &serde_json::Value| v.as_str().map(|s| s.trim_matches(['<', '>']).to_string());
    let from_header = || headers.get("x-message-id").and_then(|h| h.to_str().ok()).map(String::from);
    match api {
        Api::Resend | Api::Mailgun => s(&body["id"]),
        Api::Postmark => s(&body["MessageID"]),
        Api::Brevo => s(&body["messageId"]),
        Api::SendGrid | Api::MailerSend => from_header(),
        Api::Mailjet => s(&body["Messages"][0]["To"][0]["MessageUUID"])
            .or_else(|| body["Messages"][0]["To"][0]["MessageID"].as_i64().map(|n| n.to_string())),
    }
    .unwrap_or_default()
}

/// A readable reason from an error answer.
fn api_error_text(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let candidates = [
            &v["message"],
            &v["Message"],
            &v["error"]["message"],
            &v["error"],
            &v["errors"][0]["message"],
            &v["Messages"][0]["Errors"][0]["ErrorMessage"],
        ];
        for c in candidates {
            if let Some(s) = c.as_str() {
                return s.to_string();
            }
        }
    }
    let t = body.trim();
    if t.chars().count() > 300 {
        format!("{}…", t.chars().take(300).collect::<String>())
    } else {
        t.to_string()
    }
}

fn api_send(t: &Transport, timeout: Duration, mail: &Mail) -> Result<String, String> {
    let Transport::Api { api, key, secret, base, domain } = t else { unreachable!() };
    let api = *api;
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("fogu-mail/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| io_err!("mail: {}", e))?;
    let req = match api {
        Api::Resend => client.post(format!("{}/emails", base)).bearer_auth(key),
        Api::SendGrid => client.post(format!("{}/v3/mail/send", base)).bearer_auth(key),
        Api::Postmark => client
            .post(format!("{}/email", base))
            .header("X-Postmark-Server-Token", key)
            .header("Accept", "application/json"),
        Api::Brevo => client.post(format!("{}/v3/smtp/email", base)).header("api-key", key).header("accept", "application/json"),
        Api::MailerSend => client.post(format!("{}/v1/email", base)).bearer_auth(key),
        Api::Mailjet => client.post(format!("{}/v3.1/send", base)).basic_auth(key, secret.as_ref()),
        Api::Mailgun => client
            .post(format!("{}/v3/{}/messages", base, domain.as_deref().unwrap_or_default()))
            .basic_auth("api", Some(key)),
    };
    let req = if api == Api::Mailgun {
        req.multipart(mailgun_form(mail)?)
    } else {
        req.json(&api_payload(api, mail))
    };
    let resp = req.send().map_err(|e| io_err!("mail: {}: {}", api.name(), e))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(io_err!(
            "mail: {} rejected the message ({}): {}",
            api.name(),
            status.as_u16(),
            api_error_text(&text)
        ));
    }
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    if api == Api::Postmark && body["ErrorCode"].as_i64().is_some_and(|c| c != 0) {
        return Err(io_err!("mail: postmark rejected the message: {}", api_error_text(&text)));
    }
    Ok(api_message_id(api, &headers, &body))
}

fn mailgun_form(mail: &Mail) -> Result<reqwest::blocking::multipart::Form, String> {
    use reqwest::blocking::multipart::{Form, Part};
    let mut f = Form::new().text("from", mail.from.display()).text("subject", mail.subject.clone());
    for (key, list) in [("to", &mail.to), ("cc", &mail.cc), ("bcc", &mail.bcc)] {
        for a in list.iter() {
            f = f.text(key, a.display());
        }
    }
    if !mail.reply_to.is_empty() {
        f = f.text("h:Reply-To", joined(&mail.reply_to));
    }
    if let Some(t) = &mail.text {
        f = f.text("text", t.clone());
    }
    if let Some(h) = &mail.html {
        f = f.text("html", h.clone());
    }
    for (k, v) in &mail.headers {
        f = f.text(format!("h:{}", k), v.clone());
    }
    for a in &mail.attachments {
        let (field, name) = match &a.cid {
            Some(cid) => ("inline", cid.clone()),
            None => ("attachment", a.filename.clone()),
        };
        let part = Part::bytes(a.content.clone())
            .file_name(name)
            .mime_str(&a.content_type)
            .map_err(|_| format!("mail: invalid content type '{}'", a.content_type))?;
        f = f.part(field, part);
    }
    Ok(f)
}

// ---------------------------------------------------------------------
// Console provider
// ---------------------------------------------------------------------

/// What the console provider prints (deterministic: no date or id).
fn console_text(mail: &Mail) -> String {
    let mut out = String::from("--- mail ---\n");
    out.push_str(&format!("From: {}\n", mail.from.display()));
    for (k, v) in [("To", &mail.to), ("Cc", &mail.cc), ("Bcc", &mail.bcc), ("Reply-To", &mail.reply_to)] {
        if !v.is_empty() {
            out.push_str(&format!("{}: {}\n", k, joined(v)));
        }
    }
    out.push_str(&format!("Subject: {}\n", mail.subject));
    for (k, v) in &mail.headers {
        out.push_str(&format!("{}: {}\n", k, v));
    }
    for a in &mail.attachments {
        let inline = a.cid.as_ref().map(|c| format!(", inline cid:{}", c)).unwrap_or_default();
        out.push_str(&format!("Attachment: {} ({}, {} bytes{})\n", a.filename, a.content_type, a.content.len(), inline));
    }
    out.push('\n');
    match (&mail.text, &mail.html) {
        (Some(t), _) => out.push_str(t),
        (None, Some(h)) => out.push_str(h),
        _ => {}
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---");
    out
}

// ---------------------------------------------------------------------
// Mailer
// ---------------------------------------------------------------------

fn connect(args: &[Value]) -> Result<Value, String> {
    max_args(args, 1, "mail.connect")?;
    let config = parse_config(&args[0])?;
    Ok(NativeObjectRef::new(Mailer { config, sent: 0 }).to_value())
}

pub struct Mailer {
    config: Config,
    sent: u64,
}

fn result(id: String, provider: &str) -> Value {
    map(vec![("id", Value::Str(id.into())), ("provider", Value::Str(provider.to_string().into()))])
}

impl FoguObject for Mailer {
    fn name(&self) -> &str {
        "Mailer"
    }

    fn get(&self, field: &str) -> Option<Value> {
        let c = &self.config;
        Some(match field {
            "provider" => Value::Str(c.transport.name().into()),
            "from" => c.from.as_ref().map_or(Value::Nil, |a| Value::Str(a.display().into())),
            "sent" => Value::Int(self.sent as i64),
            "host" => match &c.transport {
                Transport::Smtp { host, .. } => Value::Str(host.clone().into()),
                Transport::Api { base, .. } => Value::Str(base.clone().into()),
                Transport::Console => Value::Nil,
            },
            "port" => match &c.transport {
                Transport::Smtp { port, .. } => Value::Int(*port as i64),
                _ => Value::Nil,
            },
            "secure" => match &c.transport {
                Transport::Smtp { security, .. } => Value::Str(
                    match security {
                        Security::Tls => "tls",
                        Security::StartTls => "starttls",
                        Security::None => "none",
                    }
                    .into(),
                ),
                _ => Value::Nil,
            },
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            // render(message) → the RFC 5322 text that SMTP would send
            "render" => {
                max_args(args, 1, "render")?;
                let mail = parse_mail(args.first().unwrap_or(&Value::Nil), &self.config.from)?;
                Ok(Value::Str(build_message(&mail)?.text.into()))
            }
            "close" => Ok(Value::Nil),
            other => Err(format!("method '{}' does not exist on Mailer (send sendMany verify render close)", other)),
        }
    }

    fn start_async(&mut self, method: &str, args: &[Value]) -> Option<Result<Poller, String>> {
        let r = (|| -> Result<Option<Poller>, String> {
            let cfg = self.config.clone();
            let provider = cfg.transport.name();
            Ok(Some(match method {
                "send" => {
                    max_args(args, 1, "send")?;
                    let mail = parse_mail(args.first().unwrap_or(&Value::Nil), &cfg.from)?;
                    self.sent += 1;
                    if let Transport::Console = cfg.transport {
                        println!("{}", console_text(&mail));
                        let id = format!("console-{}", self.sent);
                        return Ok(Some(Box::new(move || Some(Ok(result(id.clone(), "console"))))));
                    }
                    thread_poller(move || send_now(&cfg, &mail), move |id| Ok(result(id, &provider)))
                }
                // sendMany([messages]) → results, stopping at the first failure
                "sendMany" => {
                    max_args(args, 1, "sendMany")?;
                    let items = match args.first() {
                        Some(Value::List(l)) => l.borrow().clone(),
                        _ => return Err("sendMany expects a list of messages".into()),
                    };
                    let mails = items.iter().map(|m| parse_mail(m, &cfg.from)).collect::<Result<Vec<_>, _>>()?;
                    self.sent += mails.len() as u64;
                    if let Transport::Console = cfg.transport {
                        let base = self.sent - mails.len() as u64;
                        let out: Vec<Value> = mails
                            .iter()
                            .enumerate()
                            .map(|(i, m)| {
                                println!("{}", console_text(m));
                                result(format!("console-{}", base + i as u64 + 1), "console")
                            })
                            .collect();
                        let v = crate::native::util::list(out);
                        return Ok(Some(Box::new(move || Some(Ok(v.clone())))));
                    }
                    thread_poller(
                        move || mails.iter().map(|m| send_now(&cfg, m)).collect::<Result<Vec<String>, String>>(),
                        move |ids| Ok(crate::native::util::list(ids.into_iter().map(|id| result(id, &provider)).collect())),
                    )
                }
                // verify(): connects and authenticates (SMTP); APIs are checked on send
                "verify" => {
                    max_args(args, 0, "verify")?;
                    match cfg.transport {
                        Transport::Smtp { .. } => thread_poller(move || smtp_verify(&cfg.transport, cfg.timeout), |ok| Ok(Value::Bool(ok))),
                        _ => Box::new(|| Some(Ok(Value::Bool(true)))),
                    }
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

    fn methods(&self) -> Vec<&'static str> {
        vec!["send", "sendMany", "verify", "render", "close"]
    }
}

fn send_now(cfg: &Config, mail: &Mail) -> Result<String, String> {
    match &cfg.transport {
        Transport::Smtp { .. } => smtp_send(&cfg.transport, cfg.timeout, mail),
        Transport::Api { .. } => api_send(&cfg.transport, cfg.timeout, mail),
        Transport::Console => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Addr {
        parse_addr(&Value::Str(s.into()), "to").unwrap()
    }

    fn sample() -> Mail {
        Mail {
            from: addr("Shop <shop@example.com>"),
            to: vec![addr("ana@example.com"), addr("Bob <bob@example.com>")],
            cc: vec![],
            bcc: vec![addr("audit@example.com")],
            reply_to: vec![addr("help@example.com")],
            subject: "Hi".into(),
            text: Some("hello".into()),
            html: Some("<p>hello</p>".into()),
            attachments: vec![Attach {
                filename: "a.txt".into(),
                content: b"abc".to_vec(),
                content_type: "text/plain".into(),
                cid: None,
            }],
            headers: vec![("X-Tag".into(), "t1".into())],
        }
    }

    #[test]
    fn addresses() {
        let a = addr("  \"Ana, Dev\" <ana@example.com> ");
        assert_eq!(a.name.as_deref(), Some("Ana, Dev"));
        assert_eq!(a.display(), "\"Ana, Dev\" <ana@example.com>");
        assert_eq!(addr("x@y.io").display(), "x@y.io");
        assert!(parse_addr(&Value::Str("not-an-email".into()), "to").is_err());
    }

    #[test]
    fn presets_and_urls() {
        let c = parse_config(&Value::Str("smtps://u:p%40ss@mail.example.com".into())).unwrap();
        match c.transport {
            Transport::Smtp { host, port, security, user, password, .. } => {
                assert_eq!((host.as_str(), port, security), ("mail.example.com", 465, Security::Tls));
                assert_eq!((user.as_deref(), password.as_deref()), (Some("u"), Some("p@ss")));
            }
            _ => panic!(),
        }
        assert_eq!(smtp_preset("gmail", None).unwrap(), ("smtp.gmail.com".into(), 587, Security::StartTls));
        assert_eq!(smtp_preset("ses", Some("eu-west-1")).unwrap().0, "email-smtp.eu-west-1.amazonaws.com");
    }

    #[test]
    fn payloads() {
        let m = sample();
        let r = api_payload(Api::Resend, &m);
        assert_eq!(r["to"][1], "Bob <bob@example.com>");
        assert_eq!(r["attachments"][0]["content"], "YWJj");
        assert!(r.get("cc").is_none());
        let s = api_payload(Api::SendGrid, &m);
        assert_eq!(s["personalizations"][0]["bcc"][0]["email"], "audit@example.com");
        assert_eq!(s["content"][0]["type"], "text/plain");
        assert_eq!(s["reply_to"]["email"], "help@example.com");
        let p = api_payload(Api::Postmark, &m);
        assert_eq!(p["To"], "ana@example.com, Bob <bob@example.com>");
        assert_eq!(p["Headers"][0]["Name"], "X-Tag");
        let b = api_payload(Api::Brevo, &m);
        assert_eq!(b["sender"]["name"], "Shop");
        assert_eq!(b["attachment"][0]["name"], "a.txt");
        let mj = api_payload(Api::Mailjet, &m);
        assert_eq!(mj["Messages"][0]["To"][1]["Name"], "Bob");
    }

    #[test]
    fn mime() {
        let raw = build_message(&sample()).unwrap().text;
        assert!(raw.contains("Subject: Hi"));
        assert!(raw.contains("X-Tag: t1"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("filename=\"a.txt\""));
        assert!(!raw.contains("audit@example.com"), "Bcc must not appear in the message");
    }
}
