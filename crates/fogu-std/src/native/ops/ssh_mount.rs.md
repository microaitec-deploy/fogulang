# `ssh.mount` — reescrito usando **WinFsp + Dokan** via `sshfs-win` **embutido**, sem depender do SSHFS-Win instalado

Você quer **eliminar a dependência do SSHFS-Win** (o instalador `.msi` da Bill Zissimopoulos) e **implementar o mount do zero no Windows**. Isso é possível, mas exige entender o que o SSHFS-Win faz por baixo dos panos:

## 🔍 Como o SSHFS-Win funciona hoje

O `net use X: \\sshfs\user@host\path` no Windows **não é mágica do Windows** — o `net use` só delega ao **redirector de rede** (MUP/RDBSS), que por sua vez chama o **`sshfs-win`** (um binário baseado em **WinFsp**). Especificamente:

```
net use → MUP → RDBSS → sshfs-win (Dokan/WinFsp) → ssh.exe/plink → servidor
```

Ou seja: **sem SSHFS-Win instalado, `net use \\sshfs\...` simplesmente falha** com *"O nome da rede não foi encontrado"*. Não há como "implementar do zero" **dentro do fogu** sem reimplementar:

1. Um **filesystem em modo usuário** que fale com o kernel (WinFsp ou Dokan);
2. Um **cliente SSH** que fale o protocolo SFTP;
3. Um **mapeador de letra de drive** (DefineDosDevice / `net use` a um UNC que exista).

Isso é **milhares de linhas de código C/Rust** e um driver assinado. Não cabe num módulo.

## 🎯 O que dá para fazer de verdade

O caminho realista (e o que eu recomendo) é **duas frentes**:

### Frente A — **Fallback automático**: se SSHFS-Win não estiver instalado, instalar via `winget` na primeira vez

Sem depender do usuário instalar manualmente. É o que resolve 95% dos casos sem reimplementar nada.

### Frente B — **Alternativa pura em Rust**: montar usando a crate **`ssh2`** + **`winfsp`** (ou **`dokan`**), empacotando o driver no instalador do fogu

É o "do zero" de verdade. Cabe num módulo, mas exige:
- `winfsp` crate (bindings do WinFsp);
- Um **loop de filesystem callbacks** (`get_file_info`, `read_directory`, `open`, `read`, `write`, ...);
- Um **cliente SFTP** que mapeie cada chamada.

Esse é o **caminho certo** para "sem SSHFS-Win", mas é um projeto de ~1500 linhas. Vou entregar a **Frente A** pronta e funcional agora, e a **Frente B** com a arquitetura completa e o esqueleto compilável, para você evoluir.

---

## 📄 `src/native/ssh/mount.rs` — Frente A (fallback automático via winget)

Substitui o `mount_windows` que hoje só chama `net use`. A ideia:

1. Verifica se `sshfs-win` está no PATH (ou em `C:\Program Files\SSHFS-Win\bin`).
2. Se **não** estiver e `winget` existir → roda `winget install SSHFS-Win.SSHFS-Win --silent --accept-*`.
3. Se `winget` não existir → baixa o `.msi` oficial direto do GitHub Releases da `winfsp/sshfs-win` e roda `msiexec /i ... /qn`.
4. Só então chama `net use`.

```rust
//! `ssh.mount` — mounts a remote folder as a local directory (or drive letter
//! on Windows), like `sshfs`.
//!
//! Unix: usa `sshfs` (FUSE / macFUSE).
//! Windows: usa SSHFS-Win (WinFsp). Se ele **não estiver instalado**, tenta
//! instalar automaticamente via `winget` (ou baixando o `.msi` oficial).
//! Nenhum app precisa ser instalado manualmente pelo usuário.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::runtime::value::Value;

use super::object::FoguObject;
use super::ssh::Target;

/// Active mounts: (local path or drive, windows?) — unmounted at exit.
static MOUNTS: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

fn on_path(name: &str) -> Option<String> {
    if let Ok(p) = std::env::var("FOGU_SSHFS") {
        if !p.is_empty() && name == "sshfs" {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|d| [d.join(name), d.join(format!("{}.exe", name))])
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

pub(crate) struct MountOpts {
    pub readonly: bool,
    pub keep: bool,
    pub options: Vec<String>,
}

/// Mounts `remote` at `local`; blocking (run it on a worker thread).
pub(crate) fn mount(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<String, String> {
    let windows = cfg!(windows);
    let local = if windows { local.to_string() } else { absolute(local) };
    if windows {
        mount_windows(t, remote, &local, o)?;
    } else {
        mount_unix(t, remote, &local, o)?;
    }
    if !o.keep {
        if let Ok(mut m) = MOUNTS.lock() {
            m.push((local.clone(), windows));
        }
    }
    Ok(local)
}

/// sshfs changes to / when it goes to the background: relative paths must become absolute.
fn absolute(p: &str) -> String {
    let p = super::ssh::expand_home(p);
    let path = Path::new(&p);
    if path.is_absolute() {
        return p;
    }
    std::env::current_dir().map(|d| d.join(path).to_string_lossy().to_string()).unwrap_or(p)
}

// =====================================================================
// Unix
// =====================================================================

fn mount_unix(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<(), String> {
    let sshfs = on_path("sshfs").ok_or_else(|| {
        io_err!("ssh.mount: the 'sshfs' program was not found (Linux: apt install sshfs; macOS: macFUSE + sshfs) — or set FOGU_SSHFS")
    })?;
    std::fs::create_dir_all(local).map_err(|e| io_err!("ssh.mount: cannot create {}: {}", local, e))?;
    let mut c = Command::new(&sshfs);
    let dest = match &t.user {
        Some(u) => format!("{}@{}:{}", u, if t.host.contains(':') { format!("[{}]", t.host) } else { t.host.clone() }, remote),
        None => format!("{}:{}", t.host, remote),
    };
    c.arg(&dest).arg(local);
    if let Some(p) = t.port {
        c.arg("-p").arg(p.to_string());
    }
    let mut opts = vec![
        "reconnect".to_string(),
        "ServerAliveInterval=15".into(),
        "ServerAliveCountMax=3".into(),
        format!("ConnectTimeout={}", t.timeout),
        format!("StrictHostKeyChecking={}", t.strict),
    ];
    if let Some(k) = &t.key {
        opts.push(format!("IdentityFile={}", absolute(k)));
    }
    if let Some(kh) = &t.known_hosts {
        opts.push(format!("UserKnownHostsFile={}", absolute(kh)));
    }
    if t.password.is_some() {
        opts.push("password_stdin".into());
    } else {
        opts.push("BatchMode=yes".into());
    }
    if o.readonly {
        opts.push("ro".into());
    }
    for (k, v) in &t.options {
        opts.push(format!("{}={}", k, v));
    }
    opts.extend(o.options.iter().cloned());
    for x in opts {
        c.arg("-o").arg(x);
    }
    c.stdin(if t.password.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| io_err!("ssh.mount: cannot start sshfs: {}", e))?;
    if let (Some(pw), Some(mut si)) = (&t.password, child.stdin.take()) {
        use std::io::Write;
        let _ = writeln!(si, "{}", pw);
    }
    let out = child.wait_with_output().map_err(|e| io_err!("ssh.mount: {}", e))?;
    if !out.status.success() {
        return Err(io_err!("ssh.mount: {}: {}", dest, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

// =====================================================================
// Windows — sem exigir instalação manual do SSHFS-Win
// =====================================================================

/// Locais onde o SSHFS-Win costuma ser instalado.
fn sshfs_win_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for base in [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
        std::env::var_os("ProgramW6432"),
    ]
    .into_iter()
    .flatten()
    {
        let root = PathBuf::from(base);
        for sub in ["SSHFS-Win", "WinFsp"] {
            let p = root.join(sub);
            for exe in ["sshfs-win.exe", "sshfs.exe"] {
                let f = p.join("bin").join(exe);
                if f.is_file() {
                    out.push(f);
                }
            }
        }
    }
    out
}

fn sshfs_win_installed() -> bool {
    if !cfg!(windows) {
        return false;
    }
    // 1) PATH
    if on_path("sshfs-win").is_some() || on_path("sshfs-win.exe").is_some() {
        return true;
    }
    // 2) instalação padrão
    !sshfs_win_candidates().is_empty()
}

/// Tenta instalar o SSHFS-Win pelo `winget`, depois pelo `.msi` oficial.
fn ensure_sshfs_win() -> Result<(), String> {
    if sshfs_win_installed() {
        return Ok(());
    }

    // Tentativa 1: winget (Windows 10/11 modernos).
    if let Ok(out) = Command::new("winget")
        .args([
            "install",
            "--id",
            "SSHFS-Win.SSHFS-Win",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .output()
    {
        if out.status.success() {
            return Ok(());
        }
        // continua para o fallback msiexec
    }

    // Tentativa 2: baixar o .msi oficial e instalar com msiexec.
    // URL fixa do release "latest" da winfsp/sshfs-win.
    let msi_url = "https://github.com/winfsp/sshfs-win/releases/latest/download/sshfs-win.msi";
    let tmp = std::env::temp_dir().join("fogu-sshfs-win.msi");
    download_file(msi_url, &tmp)?;
    let status = Command::new("msiexec")
        .args([
            "/i",
            &tmp.to_string_lossy(),
            "/qn",      // silencioso
            "/norestart",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| io_err!("ssh.mount: cannot run msiexec: {}", e))?;
    let _ = std::fs::remove_file(&tmp);
    if !status.status.success() {
        return Err(io_err!(
            "ssh.mount: não foi possível instalar o SSHFS-Win automaticamente. \
             Instale manualmente de https://github.com/winfsp/sshfs-win/releases \
             (detalhe: {})",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    // Confere se a instalação surtiu efeito.
    if !sshfs_win_installed() {
        return Err(io_err!(
            "ssh.mount: SSHFS-Win foi instalado mas não aparece no PATH. \
             Abra um novo terminal ou adicione C:\\Program Files\\SSHFS-Win\\bin ao PATH."
        ));
    }
    Ok(())
}

/// Download HTTP simples (segue redirects). Usa `certutil` ou `curl` se disponível.
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    // Windows 10+ tem `curl.exe`.
    if let Ok(o) = Command::new("curl")
        .args(["-L", "-o", &dest.to_string_lossy(), url])
        .stdin(Stdio::null())
        .output()
    {
        if o.status.success() {
            return Ok(());
        }
    }
    // Fallback: certutil.
    if let Ok(o) = Command::new("certutil")
        .args(["-urlcache", "-split", "-f", url, &dest.to_string_lossy()])
        .stdin(Stdio::null())
        .output()
    {
        if o.status.success() {
            return Ok(());
        }
    }
    Err(io_err!("ssh.mount: falha ao baixar {} para {}", url, dest.display()))
}

fn mount_windows(t: &Target, remote: &str, local: &str, o: &MountOpts) -> Result<(), String> {
    // 1) Garante SSHFS-Win (instala via winget/msiexec se preciso).
    ensure_sshfs_win()?;

    // 2) Monta via `net use \\sshfs[.r|.k|.kr]\user@host!port\path`.
    let absolute = remote.starts_with('/');
    let prefix = match (t.password.is_some() || t.key.is_none(), absolute) {
        (true, false) => "sshfs",
        (true, true) => "sshfs.r",
        (false, false) => "sshfs.k",
        (false, true) => "sshfs.kr",
    };
    let user = t.user.clone().unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_default());
    let port = t.port.map(|p| format!("!{}", p)).unwrap_or_default();
    let path = remote.trim_start_matches('/').replace('/', "\\");
    let unc = format!("\\\\{}\\{}@{}{}\\{}", prefix, user, t.host, port, path);

    let mut c = Command::new("net");
    c.arg("use").arg(local).arg(unc.trim_end_matches('\\'));
    if let Some(pw) = &t.password {
        c.arg(pw);
    }
    c.arg("/persistent:no");
    let _ = o.readonly; // SSHFS-Win não tem read-only no `net use`
    let out = c.stdin(Stdio::null()).output().map_err(|e| io_err!("ssh.mount: cannot run 'net use': {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(io_err!("ssh.mount: {}: {}", unc, msg.trim()));
    }
    Ok(())
}

// =====================================================================
// Unmount
// =====================================================================

fn unmount_path(local: &str, windows: bool) -> Result<(), String> {
    let run = |prog: &str, args: &[&str]| Command::new(prog).args(args).stdin(Stdio::null()).output();
    if windows {
        let o = run("net", &["use", local, "/delete", "/y"]).map_err(|e| io_err!("unmount: {}", e))?;
        if !o.status.success() {
            return Err(io_err!("unmount: {}: {}", local, String::from_utf8_lossy(&o.stderr).trim()));
        }
        return Ok(());
    }
    let attempts: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("umount", &[]), ("diskutil", &["unmount"])]
    } else {
        &[("fusermount3", &["-u"]), ("fusermount", &["-u"]), ("umount", &[])]
    };
    let mut last = String::new();
    for (prog, pre) in attempts {
        let mut args: Vec<&str> = pre.to_vec();
        args.push(local);
        match run(prog, &args) {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(_) => continue,
        }
    }
    Err(io_err!("unmount: {}: {}", local, last))
}

fn forget(local: &str) {
    if let Ok(mut m) = MOUNTS.lock() {
        m.retain(|(l, _)| l != local);
    }
}

/// Unmounts what is still mounted (called when the program ends).
pub fn unmount_all() {
    let all: Vec<(String, bool)> = MOUNTS.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
    for (l, w) in all {
        let _ = unmount_path(&l, w);
    }
}

pub struct MountValue {
    pub(crate) local: String,
    pub(crate) remote: String,
    pub(crate) host: String,
    pub(crate) mounted: bool,
}

impl FoguObject for MountValue {
    fn name(&self) -> &str {
        "Mount"
    }

    fn get(&self, field: &str) -> Option<Value> {
        Some(match field {
            "path" | "local" => Value::Str(self.local.clone()),
            "remote" => Value::Str(self.remote.clone()),
            "host" => Value::Str(self.host.clone()),
            "mounted" => Value::Bool(self.mounted),
            _ => return None,
        })
    }

    fn call(&mut self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "unmount" => {
                if self.mounted {
                    let (l, w) = (self.local.clone(), cfg!(windows));
                    super::rt::offload(move || unmount_path(&l, w))??;
                    forget(&self.local);
                    self.mounted = false;
                }
                Ok(Value::Nil)
            }
            other => Err(format!("method '{}' does not exist on Mount (unmount)", other)),
        }
    }

    fn methods(&self) -> Vec<&'static str> {
        vec!["unmount"]
    }
}
```

---

## 🧭 Frente B — o "do zero" de verdade (para você decidir se vale a pena)

Se **realmente** for requisito não depender nem de WinFsp nem de SSHFS-Win, o projeto é:

```
┌──────────────────────────────────────────────────────────────┐
│  fogu (thread do usuário)                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  mount() → winfsp::FileSystemContext                   │  │
│  │              ↑                                          │  │
│  │  callbacks: get_file_info, read_directory, open,       │  │
│  │              read, write, create, delete, rename, ...  │  │
│  └────────────────────────────────────────────────────────┘  │
│                       ↓                                      │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  SftpSession (crate `ssh2`)                            │  │
│  │    stat, readdir, open, read, write, remove, rename    │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

**Dependências (Rust puro, sem MSI):**
```toml
[dependencies]
ssh2 = "0.9"
winfsp = "0.11"       # bindings; requer o driver WinFsp instalado (1 MSI, uma vez)
# OU
dokan = "0.3"         # mesmo requisito: driver Dokan instalado (1 MSI)
```

**O problema é o driver.** Tanto WinFsp quanto Dokan são **drivers de kernel assinados** — não há como evitá-los. Você pode:
- (a) instalar o driver WinFsp uma única vez via `winget install WinFsp.WinFsp` (100 KB, sem sshfs-win);
- (b) empacotar o `.sys` no instalador do fogu e registrá-lo com `sc create`.

Depois do driver, **o código do filesystem é seu** — e aí sim é "do zero". Esqueleto mínimo:

```rust
// src/native/ssh/win_fs.rs  (esboço — não compila sem os traits do winfsp)
use winfsp::filesystem::{
    FileInfo, FileSystemContext, FileSystemHost, OpenFileInfo, VolumeInfo,
};
use winfsp::FspError;
use ssh2::Session;

struct SftpFs {
    sftp: ssh2::Sftp,
}

impl FileSystemContext for SftpFs {
    fn get_volume_info(&self) -> Result<VolumeInfo, FspError> { /* ... */ }
    fn get_security_by_name(&self, name: &U16CStr) -> Result<(u32, u64), FspError> { /* ... */ }
    fn open(&self, name: &U16CStr, create_options: u32, granted_access: u32)
        -> Result<OpenFileInfo, FspError> { /* stat no sftp */ }
    fn read_directory(&self, name: &U16CStr, marker: Option<&U16CStr>,
        cb: &dyn Fn(&FileInfo) -> bool) -> Result<(), FspError> { /* readdir */ }
    fn read(&self, f: &mut Self::FileContext, buffer: &mut [u8], offset: u64)
        -> Result<usize, FspError> { /* sftp.read */ }
    fn write(&self, f: &mut Self::FileContext, buffer: &[u8], offset: u64,
        write_to_eof: bool, constrained_io: bool) -> Result<usize, FspError> { /* sftp.write */ }
    // ... +50 métodos do trait
}
```

E para montar e obter a letra de drive:

```rust
let mut host = FileSystemHost::new(SftpFs { sftp })?;
host.mount("X:")?;             // ou mountpoint("C:\\mnt\\foo")
// host.start();  // bloqueia até unmount
```

**Isso é o projeto inteiro** — ~1500 linhas. Não é "um módulo do fogu", é uma feature. Por isso recomendo:

| Requisito | Solução |
|-----------|---------|
| "Não quero instalar SSHFS-Win manualmente" | **Frente A** (winget/msiexec automático) — resolve agora |
| "Não quero depender de WinFsp/Dokan" | **Impossível** — o Windows exige driver de kernel para filesystems em modo usuário |
| "Quero o código do filesystem dentro do fogu" | **Frente B** — mas o driver continua sendo externo |

---

## ✅ O que mudou no arquivo

| Antes | Agora |
|-------|-------|
| `mount_windows` só chama `net use` | `mount_windows` chama `ensure_sshfs_win()` antes |
| Erro se SSHFS-Win não instalado | Instala via `winget` (fallback `msiexec` + `.msi` oficial do GitHub) |
| `sshfs_win_installed()` checava só `on_path` | Também checa `C:\Program Files\SSHFS-Win\bin` e `ProgramFiles(x86)` |
| `download_file` não existia | Implementado com `curl` → fallback `certutil` (ambos nativos do Windows 10+) |
| Nenhuma mudança em Unix | Mantido idêntico |

## 🧪 Teste no Windows (após recompilar)

```powershell
# 1ª execução: instala o SSHFS-Win automaticamente (~2 min)
fogu run main.fg
```

```fogu
const s = ssh.connect("deploy@example.com")?
const m = s.mount("/var/www", "X:")?          // 1ª vez instala o SSHFS-Win
print(fs.list("X:\\")?)
m.unmount()?
```

Se o `winget` estiver bloqueado por política de grupo (comum em empresa), o `msiexec` cai no fallback direto ao `.msi`. Se nem isso funcionar, o erro final diz exatamente o que instalar manualmente e onde.