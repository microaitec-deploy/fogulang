//! Public-key signatures for `fogu:crypto` (`sign`, `verify`, `jwk`) and
//! `fogu:jwt`: RSA (PKCS#1 v1.5 and PSS), ECDSA (P-256, P-384) and Ed25519,
//! computed by `ring` (already linked through rustls; constant-time signing).
//!
//! Keys come as PEM (`PUBLIC KEY`, `RSA PUBLIC KEY`, `CERTIFICATE`,
//! `PRIVATE KEY`, `RSA PRIVATE KEY`) or as JWK (RFC 7517). RSA keys under
//! 2048 bits are refused.

use base64::Engine;
use ring::signature::{self as sig, KeyPair};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SigAlg {
    Rs256,
    Rs384,
    Rs512,
    Ps256,
    Ps384,
    Ps512,
    Es256,
    Es384,
    EdDsa,
}

/// What a key is, which decides the algorithms it can be used with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Family {
    Rsa,
    P256,
    P384,
    Ed25519,
}

impl SigAlg {
    pub(crate) const NAMES: &'static str = "RS256 RS384 RS512 PS256 PS384 PS512 ES256 ES384 EdDSA";

    pub(crate) fn parse(s: &str) -> Option<SigAlg> {
        Some(match s.to_ascii_uppercase().as_str() {
            "RS256" => SigAlg::Rs256,
            "RS384" => SigAlg::Rs384,
            "RS512" => SigAlg::Rs512,
            "PS256" => SigAlg::Ps256,
            "PS384" => SigAlg::Ps384,
            "PS512" => SigAlg::Ps512,
            "ES256" => SigAlg::Es256,
            "ES384" => SigAlg::Es384,
            "EDDSA" | "ED25519" => SigAlg::EdDsa,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            SigAlg::Rs256 => "RS256",
            SigAlg::Rs384 => "RS384",
            SigAlg::Rs512 => "RS512",
            SigAlg::Ps256 => "PS256",
            SigAlg::Ps384 => "PS384",
            SigAlg::Ps512 => "PS512",
            SigAlg::Es256 => "ES256",
            SigAlg::Es384 => "ES384",
            SigAlg::EdDsa => "EdDSA",
        }
    }

    pub(crate) fn family(self) -> Family {
        match self {
            SigAlg::Rs256 | SigAlg::Rs384 | SigAlg::Rs512 | SigAlg::Ps256 | SigAlg::Ps384 | SigAlg::Ps512 => Family::Rsa,
            SigAlg::Es256 => Family::P256,
            SigAlg::Es384 => Family::P384,
            SigAlg::EdDsa => Family::Ed25519,
        }
    }
}

impl Family {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Family::Rsa => "an RSA",
            Family::P256 => "a P-256",
            Family::P384 => "a P-384",
            Family::Ed25519 => "an Ed25519",
        }
    }

    /// The algorithm a key of this family signs with when none is named.
    pub(crate) fn default_alg(self) -> SigAlg {
        match self {
            Family::Rsa => SigAlg::Rs256,
            Family::P256 => SigAlg::Es256,
            Family::P384 => SigAlg::Es384,
            Family::Ed25519 => SigAlg::EdDsa,
        }
    }
}

/// A public key, in the form ring verifies with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PublicKey {
    /// Big-endian modulus and exponent, without leading zeros.
    Rsa { n: Vec<u8>, e: Vec<u8> },
    /// An uncompressed point (`04 || x || y`).
    Ec { curve: Family, point: Vec<u8> },
    Ed25519(Vec<u8>),
}

impl PublicKey {
    pub(crate) fn family(&self) -> Family {
        match self {
            PublicKey::Rsa { .. } => Family::Rsa,
            PublicKey::Ec { curve, .. } => *curve,
            PublicKey::Ed25519(_) => Family::Ed25519,
        }
    }
}

/// A key of a JWK set, with what the set says about it.
#[derive(Clone, Debug)]
pub(crate) struct Jwk {
    pub kid: Option<String>,
    pub alg: Option<String>,
    pub use_: Option<String>,
    pub key: PublicKey,
}

// ---------------------------------------------------------------- DER

/// A minimal DER reader: what SubjectPublicKeyInfo, PKCS#1, PKCS#8 and
/// certificates need (tags, lengths, nesting).
struct Der<'a>(&'a [u8]);

impl<'a> Der<'a> {
    /// The next element: (tag, contents, the whole element).
    fn next(&mut self) -> Option<(u8, &'a [u8], &'a [u8])> {
        let d = self.0;
        let tag = *d.first()?;
        let first = *d.get(1)?;
        let (len, head) = if first < 0x80 {
            (first as usize, 2)
        } else {
            let k = (first & 0x7f) as usize;
            if k == 0 || k > 4 {
                return None;
            }
            let mut len = 0usize;
            for i in 0..k {
                len = len << 8 | *d.get(2 + i)? as usize;
            }
            (len, 2 + k)
        };
        let end = head.checked_add(len)?;
        let body = d.get(head..end)?;
        let whole = &d[..end];
        self.0 = &d[end..];
        Some((tag, body, whole))
    }

    fn expect(&mut self, tag: u8) -> Option<&'a [u8]> {
        let (t, body, _) = self.next()?;
        (t == tag).then_some(body)
    }
}

const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_RSA_PSS: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a];
const OID_EC: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_ED25519: &[u8] = &[0x2b, 0x65, 0x70];

fn strip_zeros(b: &[u8]) -> Vec<u8> {
    let i = b.iter().position(|&x| x != 0).unwrap_or(b.len());
    b[i..].to_vec()
}

/// The family of an `AlgorithmIdentifier` (its OID and parameters).
fn family_of(oid: &[u8], param: Option<(u8, &[u8])>) -> Result<Family, String> {
    match oid {
        OID_RSA | OID_RSA_PSS => Ok(Family::Rsa),
        OID_EC => match param {
            Some((0x06, OID_P256)) => Ok(Family::P256),
            Some((0x06, OID_P384)) => Ok(Family::P384),
            _ => Err("unsupported elliptic curve (P-256 and P-384 are supported)".into()),
        },
        OID_ED25519 => Ok(Family::Ed25519),
        _ => Err("unsupported key type (RSA, EC P-256/P-384 and Ed25519 are supported)".into()),
    }
}

/// `RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }`.
fn rsa_pkcs1(der: &[u8]) -> Result<PublicKey, String> {
    let bad = || "not an RSA public key".to_string();
    let mut s = Der(Der(der).expect(0x30).ok_or_else(bad)?);
    let n = strip_zeros(s.expect(0x02).ok_or_else(bad)?);
    let e = strip_zeros(s.expect(0x02).ok_or_else(bad)?);
    rsa_key(n, e)
}

fn rsa_key(n: Vec<u8>, e: Vec<u8>) -> Result<PublicKey, String> {
    let bits = n.first().map_or(0, |b| (n.len() - 1) * 8 + (8 - b.leading_zeros() as usize));
    if bits < 2048 {
        return Err(format!("RSA keys under 2048 bits are not accepted (this one has {})", bits));
    }
    if bits > 8192 || e.is_empty() || e.len() > 4 {
        return Err("unsupported RSA key (2048 to 8192 bits, exponent of at most 32 bits)".into());
    }
    Ok(PublicKey::Rsa { n, e })
}

fn ec_point(curve: Family, point: &[u8]) -> Result<PublicKey, String> {
    let len = if curve == Family::P256 { 65 } else { 97 };
    if point.len() != len || point[0] != 4 {
        return Err("unsupported EC public key (an uncompressed point is expected)".into());
    }
    Ok(PublicKey::Ec { curve, point: point.to_vec() })
}

/// `SubjectPublicKeyInfo` (a `PUBLIC KEY` PEM).
fn spki(der: &[u8]) -> Result<PublicKey, String> {
    let bad = || "not a public key (SubjectPublicKeyInfo)".to_string();
    let mut outer = Der(Der(der).expect(0x30).ok_or_else(bad)?);
    let mut alg = Der(outer.expect(0x30).ok_or_else(bad)?);
    let oid = alg.expect(0x06).ok_or_else(bad)?;
    let param = alg.next().map(|(t, b, _)| (t, b));
    let bits = outer.expect(0x03).ok_or_else(bad)?;
    let (&unused, key) = bits.split_first().ok_or_else(bad)?;
    if unused != 0 {
        return Err(bad());
    }
    match family_of(oid, param)? {
        Family::Rsa => rsa_pkcs1(key),
        Family::Ed25519 if key.len() == 32 => Ok(PublicKey::Ed25519(key.to_vec())),
        Family::Ed25519 => Err(bad()),
        curve => ec_point(curve, key),
    }
}

/// The key of an X.509 certificate (`tbsCertificate.subjectPublicKeyInfo`).
fn cert_key(der: &[u8]) -> Result<PublicKey, String> {
    let bad = || "not an X.509 certificate".to_string();
    let mut cert = Der(Der(der).expect(0x30).ok_or_else(bad)?);
    let mut tbs = Der(cert.expect(0x30).ok_or_else(bad)?);
    let (tag, _, _) = tbs.next().ok_or_else(bad)?;
    if tag == 0xa0 {
        tbs.next().ok_or_else(bad)?; // the serial number, after the version
    }
    for _ in 0..4 {
        tbs.next().ok_or_else(bad)?; // signature algorithm, issuer, validity, subject
    }
    let (_, _, whole) = tbs.next().ok_or_else(bad)?;
    spki(whole)
}

// ---------------------------------------------------------------- PEM

/// The first PEM block of `text`: (label, DER).
fn pem(text: &str) -> Result<(String, Vec<u8>), String> {
    let start = text.find("-----BEGIN ").ok_or("not a PEM key (no -----BEGIN line)")?;
    let rest = &text[start + 11..];
    let label_end = rest.find("-----").ok_or("malformed PEM")?;
    let label = rest[..label_end].trim().to_string();
    let body = &rest[label_end + 5..];
    let end = body.find("-----END").ok_or("malformed PEM (no -----END line)")?;
    let b64: String = body[..end].chars().filter(|c| !c.is_whitespace()).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()).map_err(|_| "malformed PEM (bad base64)".to_string())?;
    Ok((label, der))
}

pub(crate) fn is_pem(text: &str) -> bool {
    text.trim_start().starts_with("-----BEGIN ")
}

/// A public key from PEM: a public key, a certificate, or the public half of a private key.
pub(crate) fn public_from_pem(text: &str) -> Result<PublicKey, String> {
    let (label, der) = pem(text)?;
    match label.as_str() {
        "PUBLIC KEY" => spki(&der),
        "RSA PUBLIC KEY" => rsa_pkcs1(&der),
        "CERTIFICATE" => cert_key(&der),
        "PRIVATE KEY" | "RSA PRIVATE KEY" => Private::parse(&label, &der)?.public(),
        other => Err(format!("unsupported PEM block '{}' (PUBLIC KEY, RSA PUBLIC KEY, CERTIFICATE, PRIVATE KEY)", other)),
    }
}

// ---------------------------------------------------------------- private keys

enum Private {
    Rsa(sig::RsaKeyPair),
    /// PKCS#8, parsed with the algorithm when signing (ring binds the two).
    Ec(Family, Vec<u8>),
    Ed25519(sig::Ed25519KeyPair),
}

impl Private {
    fn parse(label: &str, der: &[u8]) -> Result<Private, String> {
        let rejected = |e: ring::error::KeyRejected| format!("the private key was rejected: {}", e);
        if label == "RSA PRIVATE KEY" {
            return sig::RsaKeyPair::from_der(der).map(Private::Rsa).map_err(rejected);
        }
        if label != "PRIVATE KEY" {
            return Err(format!(
                "unsupported private key '{}' (PKCS#8 'PRIVATE KEY' or 'RSA PRIVATE KEY'; convert with `openssl pkcs8 -topk8 -nocrypt`)",
                label
            ));
        }
        // PrivateKeyInfo ::= SEQUENCE { version, AlgorithmIdentifier, OCTET STRING, ... }
        let bad = || "not a PKCS#8 private key".to_string();
        let mut info = Der(Der(der).expect(0x30).ok_or_else(bad)?);
        info.expect(0x02).ok_or_else(bad)?;
        let mut alg = Der(info.expect(0x30).ok_or_else(bad)?);
        let oid = alg.expect(0x06).ok_or_else(bad)?;
        let param = alg.next().map(|(t, b, _)| (t, b));
        match family_of(oid, param)? {
            Family::Rsa => sig::RsaKeyPair::from_pkcs8(der).map(Private::Rsa).map_err(rejected),
            Family::Ed25519 => sig::Ed25519KeyPair::from_pkcs8_maybe_unchecked(der).map(Private::Ed25519).map_err(rejected),
            curve => {
                // parsed once here so that a bad key fails now, not at the first signature
                ec_pair(curve, der)?;
                Ok(Private::Ec(curve, der.to_vec()))
            }
        }
    }

    fn family(&self) -> Family {
        match self {
            Private::Rsa(_) => Family::Rsa,
            Private::Ec(c, _) => *c,
            Private::Ed25519(_) => Family::Ed25519,
        }
    }

    fn public(&self) -> Result<PublicKey, String> {
        match self {
            Private::Rsa(k) => rsa_pkcs1(k.public_key().as_ref()),
            Private::Ec(curve, der) => ec_point(*curve, ec_pair(*curve, der)?.public_key().as_ref()),
            Private::Ed25519(k) => Ok(PublicKey::Ed25519(k.public_key().as_ref().to_vec())),
        }
    }
}

fn ec_pair(curve: Family, der: &[u8]) -> Result<sig::EcdsaKeyPair, String> {
    let alg = if curve == Family::P256 { &sig::ECDSA_P256_SHA256_FIXED_SIGNING } else { &sig::ECDSA_P384_SHA384_FIXED_SIGNING };
    sig::EcdsaKeyPair::from_pkcs8(alg, der, &ring::rand::SystemRandom::new()).map_err(|e| format!("the private key was rejected: {}", e))
}

/// The family of a private key in PEM (to pick the default algorithm).
pub(crate) fn private_family(text: &str) -> Result<Family, String> {
    let (label, der) = pem(text)?;
    Ok(Private::parse(&label, &der)?.family())
}

/// Signs `msg` with a PEM private key. ECDSA signatures are `r || s` (the JOSE form).
pub(crate) fn sign(alg: SigAlg, private_pem: &str, msg: &[u8]) -> Result<Vec<u8>, String> {
    let (label, der) = pem(private_pem)?;
    let key = Private::parse(&label, &der)?;
    if key.family() != alg.family() {
        return Err(format!("{} key cannot sign {}", key.family().label(), alg.name()));
    }
    let rng = ring::rand::SystemRandom::new();
    match key {
        Private::Rsa(k) => {
            let enc: &dyn sig::RsaEncoding = match alg {
                SigAlg::Rs256 => &sig::RSA_PKCS1_SHA256,
                SigAlg::Rs384 => &sig::RSA_PKCS1_SHA384,
                SigAlg::Rs512 => &sig::RSA_PKCS1_SHA512,
                SigAlg::Ps256 => &sig::RSA_PSS_SHA256,
                SigAlg::Ps384 => &sig::RSA_PSS_SHA384,
                _ => &sig::RSA_PSS_SHA512,
            };
            let mut out = vec![0; k.public().modulus_len()];
            k.sign(enc, &rng, msg, &mut out).map_err(|_| "RSA signing failed".to_string())?;
            Ok(out)
        }
        Private::Ec(curve, der) => Ok(ec_pair(curve, &der)?.sign(&rng, msg).map_err(|_| "ECDSA signing failed".to_string())?.as_ref().to_vec()),
        Private::Ed25519(k) => Ok(k.sign(msg).as_ref().to_vec()),
    }
}

/// Checks a signature; `Err` when the key cannot be used with `alg`. ECDSA
/// signatures may be `r || s` (JOSE) or DER.
pub(crate) fn verify(alg: SigAlg, key: &PublicKey, msg: &[u8], signature: &[u8]) -> Result<bool, String> {
    if key.family() != alg.family() {
        return Err(format!("{} key cannot verify {}", key.family().label(), alg.name()));
    }
    Ok(match key {
        PublicKey::Rsa { n, e } => {
            let params: &sig::RsaParameters = match alg {
                SigAlg::Rs256 => &sig::RSA_PKCS1_2048_8192_SHA256,
                SigAlg::Rs384 => &sig::RSA_PKCS1_2048_8192_SHA384,
                SigAlg::Rs512 => &sig::RSA_PKCS1_2048_8192_SHA512,
                SigAlg::Ps256 => &sig::RSA_PSS_2048_8192_SHA256,
                SigAlg::Ps384 => &sig::RSA_PSS_2048_8192_SHA384,
                _ => &sig::RSA_PSS_2048_8192_SHA512,
            };
            sig::RsaPublicKeyComponents { n: n.as_slice(), e: e.as_slice() }.verify(params, msg, signature).is_ok()
        }
        PublicKey::Ec { curve, point } => {
            let fixed = signature.len() == if *curve == Family::P256 { 64 } else { 96 };
            let a: &'static dyn sig::VerificationAlgorithm = match (curve, fixed) {
                (Family::P256, true) => &sig::ECDSA_P256_SHA256_FIXED,
                (Family::P256, false) => &sig::ECDSA_P256_SHA256_ASN1,
                (_, true) => &sig::ECDSA_P384_SHA384_FIXED,
                (_, false) => &sig::ECDSA_P384_SHA384_ASN1,
            };
            sig::UnparsedPublicKey::new(a, point).verify(msg, signature).is_ok()
        }
        PublicKey::Ed25519(k) => sig::UnparsedPublicKey::new(&sig::ED25519, k).verify(msg, signature).is_ok(),
    })
}

// ---------------------------------------------------------------- JWK

fn b64url(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

fn b64url_encode(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// A JWK (RFC 7517/7518/8037) → its key. Symmetric (`oct`) keys are not public keys.
pub(crate) fn jwk_from_json(j: &serde_json::Value) -> Result<Jwk, String> {
    let s = |k: &str| j.get(k).and_then(|v| v.as_str());
    let bytes = |k: &str| s(k).and_then(b64url).ok_or_else(|| format!("JWK: missing or invalid '{}'", k));
    let key = match s("kty") {
        Some("RSA") => rsa_key(strip_zeros(&bytes("n")?), strip_zeros(&bytes("e")?))?,
        Some("EC") => {
            let (curve, size) = match s("crv") {
                Some("P-256") => (Family::P256, 32),
                Some("P-384") => (Family::P384, 48),
                other => return Err(format!("JWK: unsupported curve {:?} (P-256, P-384)", other.unwrap_or(""))),
            };
            let (x, y) = (bytes("x")?, bytes("y")?);
            if x.len() != size || y.len() != size {
                return Err("JWK: 'x' and 'y' do not match the curve".into());
            }
            let mut point = vec![4];
            point.extend(x);
            point.extend(y);
            ec_point(curve, &point)?
        }
        Some("OKP") if s("crv") == Some("Ed25519") => {
            let x = bytes("x")?;
            if x.len() != 32 {
                return Err("JWK: an Ed25519 'x' has 32 bytes".into());
            }
            PublicKey::Ed25519(x)
        }
        Some("OKP") => return Err(format!("JWK: unsupported curve {:?} (Ed25519)", s("crv").unwrap_or(""))),
        Some("oct") => return Err("JWK: a symmetric ('oct') key is not a public key".into()),
        other => return Err(format!("JWK: unsupported key type {:?} (RSA, EC, OKP)", other.unwrap_or(""))),
    };
    Ok(Jwk { kid: s("kid").map(str::to_string), alg: s("alg").map(str::to_string), use_: s("use").map(str::to_string), key })
}

/// A public key → its JWK members (`kty` and the key; the caller adds `kid`, `alg`, `use`).
pub(crate) fn jwk_members(key: &PublicKey) -> Vec<(&'static str, String)> {
    match key {
        PublicKey::Rsa { n, e } => vec![("kty", "RSA".into()), ("n", b64url_encode(n)), ("e", b64url_encode(e))],
        PublicKey::Ec { curve, point } => {
            let size = (point.len() - 1) / 2;
            vec![
                ("kty", "EC".into()),
                ("crv", if *curve == Family::P256 { "P-256" } else { "P-384" }.into()),
                ("x", b64url_encode(&point[1..1 + size])),
                ("y", b64url_encode(&point[1 + size..])),
            ]
        }
        PublicKey::Ed25519(x) => vec![("kty", "OKP".into()), ("crv", "Ed25519".into()), ("x", b64url_encode(x))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7515_es256_and_rs256_examples() {
        // RFC 7515 appendix A.3: an ES256 JWS
        let jwk = serde_json::json!({ "kty": "EC", "crv": "P-256",
            "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU", "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0" });
        let k = jwk_from_json(&jwk).unwrap();
        let input = b"eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
        let s = b64url("DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q").unwrap();
        assert!(verify(SigAlg::Es256, &k.key, input, &s).unwrap());
        assert!(!verify(SigAlg::Es256, &k.key, b"other", &s).unwrap());
        assert!(verify(SigAlg::Rs256, &k.key, input, &s).is_err());
    }

    #[test]
    fn small_rsa_keys_are_refused() {
        assert!(rsa_key(vec![0xff; 128], vec![1, 0, 1]).is_err());
        assert!(rsa_key(vec![0xff; 256], vec![1, 0, 1]).is_ok());
    }
}
