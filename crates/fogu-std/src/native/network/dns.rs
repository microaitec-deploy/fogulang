//! A small DNS client over the runtime's sockets (UDP with EDNS0, TCP when
//! the answer is truncated), used by `fogu:security` (`dns`, `reverseDns`).
//!
//! Servers: `{ server }`, else `FOGU_DNS` ("1.1.1.1" or "1.1.1.1,8.8.8.8"),
//! else the system's (`/etc/resolv.conf`; on Windows the adapters' DNS
//! servers), else 1.1.1.1 and 8.8.8.8.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::OnceLock;
use std::time::Duration;

/// One answer record: its type name, its value as text, its TTL.
pub struct Record {
    pub rtype: String,
    pub value: String,
    pub ttl: u32,
}

const TYPES: &[(&str, u16)] = &[
    ("A", 1),
    ("NS", 2),
    ("CNAME", 5),
    ("SOA", 6),
    ("PTR", 12),
    ("MX", 15),
    ("TXT", 16),
    ("AAAA", 28),
    ("SRV", 33),
    ("NAPTR", 35),
    ("DS", 43),
    ("DNSKEY", 48),
    ("HTTPS", 65),
    ("CAA", 257),
    ("ANY", 255),
];

pub fn type_code(name: &str) -> Option<u16> {
    let n = name.to_ascii_uppercase();
    TYPES.iter().find(|(t, _)| *t == n).map(|(_, c)| *c).or_else(|| n.strip_prefix("TYPE").and_then(|d| d.parse().ok()))
}

fn type_name(code: u16) -> String {
    TYPES.iter().find(|(_, c)| *c == code).map(|(t, _)| t.to_string()).unwrap_or_else(|| format!("TYPE{}", code))
}

/// `1.2.3.4` → `4.3.2.1.in-addr.arpa`; IPv6 → nibbles under `ip6.arpa`.
pub fn reverse_name(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v) => {
            let mut s = String::new();
            for b in v.octets().iter().rev() {
                s.push_str(&format!("{:x}.{:x}.", b & 0xf, b >> 4));
            }
            s + "ip6.arpa"
        }
    }
}

fn system_servers() -> Vec<IpAddr> {
    let mut out = Vec::new();
    #[cfg(unix)]
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in text.lines() {
            let mut w = line.split_whitespace();
            if w.next() == Some("nameserver") {
                if let Some(ip) = w.next().and_then(|a| a.split('%').next()).and_then(|a| a.parse().ok()) {
                    out.push(ip);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        let ps = "Get-DnsClientServerAddress -AddressFamily IPv4,IPv6 | ForEach-Object { $_.ServerAddresses }";
        if let Ok(o) = std::process::Command::new("powershell").args(["-NoProfile", "-Command", ps]).output() {
            for l in String::from_utf8_lossy(&o.stdout).lines() {
                if let Ok(ip) = l.trim().parse::<IpAddr>() {
                    if !out.contains(&ip) {
                        out.push(ip);
                    }
                }
            }
        }
    }
    out
}

/// The servers to ask, in order.
pub fn servers(explicit: Option<&str>) -> Vec<SocketAddr> {
    static SYSTEM: OnceLock<Vec<IpAddr>> = OnceLock::new();
    let parse = |list: &str| -> Vec<SocketAddr> {
        list.split(',')
            .filter_map(|s| {
                let s = s.trim();
                s.parse::<SocketAddr>().ok().or_else(|| s.trim_matches(['[', ']']).parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, 53)))
            })
            .collect()
    };
    if let Some(e) = explicit {
        return parse(e);
    }
    if let Ok(e) = std::env::var("FOGU_DNS") {
        let v = parse(&e);
        if !v.is_empty() {
            return v;
        }
    }
    let sys = SYSTEM.get_or_init(system_servers);
    if !sys.is_empty() {
        return sys.iter().map(|ip| SocketAddr::new(*ip, 53)).collect();
    }
    vec!["1.1.1.1:53".parse().expect("valid"), "8.8.8.8:53".parse().expect("valid")]
}

fn query(id: u16, name: &str, qtype: u16) -> Result<Vec<u8>, String> {
    let mut q = Vec::with_capacity(64);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // RD
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 1]); // 1 question, 1 additional (OPT)
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("invalid DNS name '{}'", name));
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&[0, 1]); // IN
    // EDNS0: answers up to 1232 bytes over UDP
    q.extend_from_slice(&[0, 0, 41, 0x04, 0xd0, 0, 0, 0, 0, 0, 0]);
    Ok(q)
}

struct Msg<'a> {
    b: &'a [u8],
}

impl Msg<'_> {
    fn u16(&self, at: usize) -> Result<u16, String> {
        self.b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).ok_or_else(|| "truncated answer".to_string())
    }

    fn u32(&self, at: usize) -> Result<u32, String> {
        self.b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "truncated answer".to_string())
    }

    /// A (maybe compressed) name at `at` → (name with a trailing dot, the offset after it).
    fn name(&self, mut at: usize) -> Result<(String, usize), String> {
        let mut out = String::new();
        let mut end = None;
        for _ in 0..128 {
            let len = *self.b.get(at).ok_or("truncated name")? as usize;
            if len == 0 {
                if out.is_empty() {
                    out.push('.');
                }
                return Ok((out, end.unwrap_or(at + 1)));
            }
            if len & 0xc0 == 0xc0 {
                let ptr = ((len & 0x3f) << 8) | *self.b.get(at + 1).ok_or("truncated name")? as usize;
                end.get_or_insert(at + 2);
                at = ptr;
                continue;
            }
            let label = self.b.get(at + 1..at + 1 + len).ok_or("truncated name")?;
            out.push_str(&String::from_utf8_lossy(label));
            out.push('.');
            at += 1 + len;
        }
        Err("name compression loop".into())
    }

    fn data(&self, rtype: u16, at: usize, len: usize) -> Result<String, String> {
        let d = self.b.get(at..at + len).ok_or("truncated record")?;
        Ok(match rtype {
            1 if len == 4 => IpAddr::from([d[0], d[1], d[2], d[3]]).to_string(),
            28 if len == 16 => IpAddr::from(<[u8; 16]>::try_from(d).expect("16 bytes")).to_string(),
            2 | 5 | 12 => self.name(at)?.0,
            15 => format!("{} {}", self.u16(at)?, self.name(at + 2)?.0),
            6 => {
                let (m, a) = self.name(at)?;
                let (r, a) = self.name(a)?;
                format!("{} {} {} {} {} {} {}", m, r, self.u32(a)?, self.u32(a + 4)?, self.u32(a + 8)?, self.u32(a + 12)?, self.u32(a + 16)?)
            }
            33 => format!("{} {} {} {}", self.u16(at)?, self.u16(at + 2)?, self.u16(at + 4)?, self.name(at + 6)?.0),
            // TXT: its character strings, joined
            16 => {
                let mut s = String::new();
                let mut i = 0;
                while i < d.len() {
                    let l = d[i] as usize;
                    s.push_str(&String::from_utf8_lossy(d.get(i + 1..i + 1 + l).unwrap_or(&[])));
                    i += 1 + l;
                }
                s
            }
            257 if len >= 2 => {
                let tl = d[1] as usize;
                let tag = String::from_utf8_lossy(d.get(2..2 + tl).unwrap_or(&[]));
                format!("{} {} \"{}\"", d[0], tag, String::from_utf8_lossy(d.get(2 + tl..).unwrap_or(&[])))
            }
            _ => d.iter().map(|b| format!("{:02x}", b)).collect(),
        })
    }
}

/// Answers of type `qtype` (CNAMEs too when asked for them or for ANY).
fn parse(b: &[u8], id: u16, qtype: u16, name: &str, tname: &str) -> Result<(bool, Vec<Record>), String> {
    let m = Msg { b };
    if m.u16(0)? != id {
        return Err("mismatched answer".into());
    }
    let flags = m.u16(2)?;
    let truncated = flags & 0x0200 != 0;
    match flags & 0x000f {
        0 => {}
        3 => return Err(format!("{} {}: no such domain (NXDOMAIN)", name, tname)),
        2 => return Err(format!("{} {}: server failure (SERVFAIL)", name, tname)),
        5 => return Err(format!("{} {}: refused", name, tname)),
        c => return Err(format!("{} {}: error code {}", name, tname, c)),
    }
    let (qd, an) = (m.u16(4)?, m.u16(6)?);
    let mut at = 12;
    for _ in 0..qd {
        at = m.name(at)?.1 + 4;
    }
    let mut out = Vec::new();
    for _ in 0..an {
        let (_, a) = m.name(at)?;
        let rtype = m.u16(a)?;
        let ttl = m.u32(a + 4)?;
        let len = m.u16(a + 8)? as usize;
        let data_at = a + 10;
        if rtype == qtype || qtype == 255 {
            out.push(Record { rtype: type_name(rtype), value: m.data(rtype, data_at, len)?, ttl });
        }
        at = data_at + len;
    }
    Ok((truncated, out))
}

fn ask_udp(server: SocketAddr, q: &[u8], timeout: Duration) -> std::io::Result<Vec<u8>> {
    let local: SocketAddr = if server.is_ipv4() { "0.0.0.0:0".parse().expect("valid") } else { "[::]:0".parse().expect("valid") };
    let sock = UdpSocket::bind(local)?;
    sock.set_read_timeout(Some(timeout))?;
    sock.connect(server)?;
    sock.send(q)?;
    let mut buf = vec![0u8; 4096];
    let n = sock.recv(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

fn ask_tcp(server: SocketAddr, q: &[u8], timeout: Duration) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect_timeout(&server, timeout)?;
    s.set_read_timeout(Some(timeout))?;
    let mut msg = (q.len() as u16).to_be_bytes().to_vec();
    msg.extend_from_slice(q);
    s.write_all(&msg)?;
    let mut len = [0u8; 2];
    s.read_exact(&mut len)?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// Resolves `name` (records of type `rtype`), trying each server in turn.
pub fn resolve(name: &str, rtype: &str, server: Option<&str>, timeout: Duration) -> Result<Vec<Record>, String> {
    let qtype = type_code(rtype).ok_or_else(|| format!("unknown record type '{}'", rtype))?;
    let tname = rtype.to_ascii_uppercase();
    let mut id_bytes = [0u8; 2];
    let _ = getrandom::getrandom(&mut id_bytes);
    let id = u16::from_be_bytes(id_bytes);
    let q = query(id, name, qtype)?;
    let mut last = format!("{} {}: no DNS server answered", name, tname);
    for srv in servers(server) {
        let answer = match ask_udp(srv, &q, timeout) {
            Ok(a) => a,
            Err(e) => {
                last = format!("{} {}: {}: {}", name, tname, srv, e);
                continue;
            }
        };
        let (truncated, records) = match parse(&answer, id, qtype, name, &tname) {
            Ok(r) => r,
            Err(e) if e.contains("NXDOMAIN") => return Err(e),
            Err(e) => {
                last = e;
                continue;
            }
        };
        // too big for UDP: the same question over TCP
        let records = if truncated {
            match ask_tcp(srv, &q, timeout).map_err(|e| e.to_string()).and_then(|a| parse(&a, id, qtype, name, &tname)) {
                Ok((_, r)) => r,
                Err(e) => {
                    last = e;
                    continue;
                }
            }
        } else {
            records
        };
        if records.is_empty() {
            return Err(format!("{} {}: no records", name, tname));
        }
        return Ok(records);
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_queries() {
        assert_eq!(reverse_name("8.8.4.4".parse().unwrap()), "4.4.8.8.in-addr.arpa");
        assert!(reverse_name("2001:db8::1".parse().unwrap()).ends_with(".8.b.d.0.1.0.0.2.ip6.arpa"));
        let q = query(7, "example.com", 1).unwrap();
        assert_eq!(&q[12..25], b"\x07example\x03com\x00");
        assert_eq!(type_code("mx"), Some(15));
        assert_eq!(type_code("TYPE99"), Some(99));
    }

    #[test]
    fn parses_compressed_answers() {
        // header, question example.com A, answer: pointer to the question name, A 93.184.216.34
        let mut b = vec![0, 7, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        b.extend_from_slice(b"\x07example\x03com\x00\x00\x01\x00\x01");
        b.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0x0e, 0x10, 0, 4, 93, 184, 216, 34]);
        let (tc, r) = parse(&b, 7, 1, "example.com", "A").unwrap();
        assert!(!tc);
        assert_eq!((r[0].rtype.as_str(), r[0].value.as_str(), r[0].ttl), ("A", "93.184.216.34", 3600));
    }
}
