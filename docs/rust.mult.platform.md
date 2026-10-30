# Rust Cross-Compilation para Linux e Raspberry Pi

Guia para desenvolver aplicações em Rust e gerar executáveis nativos para diferentes sistemas Linux e arquitecturas, incluindo Raspberry Pi.

## 1. Objectivo

O Rust permite desenvolver aplicações multiplataforma e compilá-las para diferentes sistemas operativos e arquitecturas.

Com este guia, podes gerar executáveis para:

- Linux x86-64 (Intel/AMD).
- Linux ARM64 (AArch64).
- Raspberry Pi com Linux de 32 bits (ARMv7).
- Raspberry Pi com Linux de 64 bits (ARM64).

A compilação pode ser efectuada no próprio dispositivo ou através de cross-compilation a partir de outro sistema operativo.

## 2. Pré-requisitos

Instala o Rust através do rustup.

Consulta a documentação oficial:

https://www.rust-lang.org/tools/install

Verifica a instalação:

```bash
rustc --version
cargo --version
rustup --version
```

## 3. Plataformas de destino

| Plataforma | Target Rust |
|---|---|
| Linux x86-64 (GNU) | `x86_64-unknown-linux-gnu` |
| Linux x86-64 (musl) | `x86_64-unknown-linux-musl` |
| Linux ARM64 (GNU) | `aarch64-unknown-linux-gnu` |
| Linux ARMv7 (hard-float GNU) | `armv7-unknown-linux-gnueabihf` |

### Qual target utilizar?

**Linux x86-64**

Para servidores e computadores Linux com processadores Intel ou AMD de 64 bits.

**Linux ARM64**

Para sistemas Linux de 64 bits com processadores ARM, incluindo Raspberry Pi compatíveis.

**Linux ARMv7**

Para sistemas Linux de 32 bits com arquitectura ARMv7, como o Raspberry Pi 3 Model B+.

> O target deve corresponder à arquitectura e à ABI do sistema de destino.

## 4. Compilar para Linux x86-64

Instala o target:

```bash
rustup target add x86_64-unknown-linux-gnu
```

Compila a aplicação:

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

O executável será gerado em:

```text
target/x86_64-unknown-linux-gnu/release/
```

Este executável destina-se a sistemas Linux x86-64 compatíveis.

### Nota sobre Windows

Se estiveres a desenvolver no Windows, a instalação do target não configura automaticamente um ambiente de compilação Linux.

Poderás precisar de um linker Linux, de ferramentas de compilação cruzada ou de um ambiente Linux, como o WSL2.

## 5. Compilar para Raspberry Pi com Linux de 32 bits

Para um Raspberry Pi com sistema operativo Linux ARMv7 de 32 bits, utiliza:

```bash
rustup target add armv7-unknown-linux-gnueabihf
```

Compila a aplicação:

```bash
cargo build --release --target armv7-unknown-linux-gnueabihf
```

O executável será gerado em:

```text
target/armv7-unknown-linux-gnueabihf/release/
```

### Requisitos adicionais

Para cross-compilation, poderás precisar de:

- Um linker compatível com ARMv7.
- Bibliotecas de desenvolvimento adequadas.
- Uma toolchain com suporte a Linux ARM hard-float.
- Configuração adicional para dependências nativas utilizadas pelo projecto.

## 6. Compilar para Raspberry Pi com Linux de 64 bits

Se o Raspberry Pi tiver um sistema operativo de 64 bits, utiliza o target AArch64.

Instala o target:

```bash
rustup target add aarch64-unknown-linux-gnu
```

Compila:

```bash
cargo build --release --target aarch64-unknown-linux-gnu
```

O executável será gerado em:

```text
target/aarch64-unknown-linux-gnu/release/
```

O Raspberry Pi 3 Model B+ suporta esta arquitectura, desde que esteja a executar um sistema operativo de 64 bits.

## 7. Identificar a arquitectura do Raspberry Pi

No Raspberry Pi, executa:

```bash
uname -m
```

Resultados comuns:

| Resultado | Arquitectura |
|---|---|
| `x86_64` | Linux x86-64 |
| `aarch64` | Linux ARM64 |
| `armv7l` | Linux ARMv7 de 32 bits |
| `armv6l` | Linux ARMv6 de 32 bits |

Para verificar a arquitectura e as características do sistema:

```bash
uname -a
```

Para consultar a versão do sistema operativo:

```bash
cat /etc/os-release
```

Estes comandos ajudam a determinar o target adequado.

## 8. Compilar directamente no Raspberry Pi

Também podes compilar o projecto no próprio Raspberry Pi, desde que o sistema operativo, a arquitectura e as ferramentas instaladas sejam compatíveis.

Depois de instalares o Rust, executa:

```bash
cargo build --release
```

O Cargo utilizará, por omissão, o target nativo do ambiente de compilação.

O executável será gerado em:

```text
target/release/
```

Esta abordagem evita a necessidade de configurar um linker de cross-compilation no computador de desenvolvimento, embora a compilação possa ser mais lenta em dispositivos com recursos limitados.

## 9. Estrutura dos ficheiros compilados

Depois de compilares para diferentes targets, a estrutura do projecto será semelhante a esta:

```text
project/
├── Cargo.toml
├── Cargo.lock
├── src/
│   └── main.rs
└── target/
    ├── release/
    ├── x86_64-unknown-linux-gnu/
    │   └── release/
    ├── armv7-unknown-linux-gnueabihf/
    │   └── release/
    └── aarch64-unknown-linux-gnu/
        └── release/
```

Cada directório de target contém os artefactos da compilação correspondente.

## 10. Exemplo de aplicação

Cria um novo projecto:

```bash
cargo new raspberry-app
cd raspberry-app
```

O ficheiro `src/main.rs` terá o seguinte conteúdo:

```rust
fn main() {
    println!("Aplicação Rust executada com sucesso!");
}
```

### Compilar para Linux x86-64

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

### Compilar para Raspberry Pi ARMv7

```bash
cargo build --release --target armv7-unknown-linux-gnueabihf
```

### Compilar para Raspberry Pi ARM64

```bash
cargo build --release --target aarch64-unknown-linux-gnu
```

Estes comandos pressupõem que os targets estão instalados e que o ambiente de compilação está correctamente configurado.

## 11. Executar no dispositivo de destino

Depois de compilares a aplicação, transfere o executável para o Raspberry Pi ou para o servidor Linux correspondente.

Por exemplo, através de SSH:

```bash
scp target/armv7-unknown-linux-gnueabihf/release/raspberry-app \
    utilizador@raspberrypi:/home/utilizador/
```

No Raspberry Pi, concede permissões de execução:

```bash
chmod +x /home/utilizador/raspberry-app
```

Executa a aplicação:

```bash
/home/utilizador/raspberry-app
```

Adapta o caminho do executável e o endereço SSH ao teu ambiente.

## 12. Problemas comuns

### Erro: linker não encontrado

**Causa:** o compilador não consegue encontrar as ferramentas necessárias para gerar o executável de destino.

**Solução:** instala e configura o linker e a toolchain adequados à arquitectura de destino.

### Erro: Exec format error

**Causa:** o executável pode ter sido compilado para uma arquitectura diferente da suportada pelo sistema de destino.

**Solução:** confirma a arquitectura com `uname -m` e recompila para o target correcto.

### Erro relacionado com glibc

**Causa:** o sistema de destino pode não disponibilizar uma versão compatível da glibc exigida pelo executável.

**Solução:** compila num ambiente compatível com o sistema de destino ou avalia a utilização do target musl, caso as dependências da aplicação o permitam.

### Erros em dependências nativas

**Causa:** uma biblioteca Rust pode depender de bibliotecas C, ferramentas externas ou componentes específicos da plataforma.

**Solução:** instala as dependências de desenvolvimento necessárias e configura a toolchain de cross-compilation correspondente.

## 13. Boas práticas

- Utiliza `cargo build --release` para gerar compilações optimizadas.
- Confirma sempre a arquitectura do sistema de destino.
- Mantém toolchains e dependências actualizadas.
- Testa os executáveis no sistema real onde serão utilizados.
- Evita assumir que um binário GNU funcionará em qualquer distribuição Linux.
- Verifica as dependências dinâmicas quando a aplicação não inicia.
- Utiliza integração contínua para compilar e testar múltiplos targets automaticamente.
- Considera o Docker ou o WSL2 para simplificar ambientes de compilação cruzada.

## 14. Recursos oficiais

- Rust: https://www.rust-lang.org/
- Rustup: https://rust-lang.github.io/rustup/
- Cargo: https://doc.rust-lang.org/cargo/
- Plataformas suportadas: https://doc.rust-lang.org/rustc/platform-support.html
- Compilação condicional: https://doc.rust-lang.org/reference/conditional-compilation.html

## Conclusão

O Rust permite gerar executáveis nativos para diferentes arquitecturas e sistemas Linux a partir de um único projecto.

Para o Raspberry Pi 3 Model B+, escolhe:

- `armv7-unknown-linux-gnueabihf` para Linux de 32 bits.
- `aarch64-unknown-linux-gnu` para Linux de 64 bits.

Para servidores Linux x86-64, utiliza `x86_64-unknown-linux-gnu`.

**Nota final:** a cross-compilation exige que o ambiente de compilação, o linker e as dependências estejam configurados para o target escolhido. Instalar um target com rustup, por si só, não garante que a compilação seja concluída com sucesso.















O erro acontece porque estás a compilar no Windows (`x86_64-pc-windows-msvc`) para Linux (`x86_64-unknown-linux-gnu`), e uma das dependências do teu projecto Rust, `openssl-sys`, precisa de encontrar uma instalação do OpenSSL compatível com o sistema de destino.

Instalar o OpenSSL normal no Windows não resolve necessariamente, porque o compilador precisa das bibliotecas e dos ficheiros de desenvolvimento para Linux.

Tens três soluções. Recomendo a primeira se queres compilar para Linux sem perder tempo a configurar dependências nativas no Windows.

## Solução 1 — Compilar através do WSL2 (recomendado)

O WSL2 permite compilar para Linux dentro de um ambiente Linux, mesmo que o teu computador principal seja Windows.

No terminal PowerShell, instala o WSL2, se ainda não estiver instalado:

```
wsl --install -d Ubuntu
```

Depois, abre o Ubuntu e instala as dependências:

```
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev
```

Instala o Rust no Ubuntu, seguindo as instruções oficiais: [https://www.rust-lang.org/tools/install](https://www.rust-lang.org/tools/install)

Entra na pasta do projecto e compila:

```
cargo build --release
```

Como estás a compilar dentro do Linux x86-64 para Linux x86-64, o Cargo utiliza normalmente o target nativo e o `openssl-sys` consegue localizar o OpenSSL instalado pelo sistema.

## Solução 2 — Compilar através do Docker

Se preferes manter o projecto no Windows e não instalar o Rust no WSL2, podes usar uma imagem Linux para compilar.

Na pasta do projecto, cria um `Dockerfile`:

```
FROM rust:bookworm

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN cargo build --release
```

Executa no PowerShell, na pasta do projecto:

```
docker build -t fogu-cli-builder .
```

Este exemplo compila para Linux x86-64, assumindo que o Docker está a executar num ambiente Linux x86-64. Para obter o executável no Windows, é necessário copiar o artefacto para fora do contentor, por exemplo através de um volume ou de uma etapa adicional no Dockerfile.

## Solução 3 — Compilar o OpenSSL juntamente com o projecto

Se queres continuar a utilizar cross-compilation, podes tentar a funcionalidade `vendored` do crate `openssl`.

No `Cargo.toml`, adiciona:

```
[dependencies]
openssl = { version = "0.10", features = ["vendored"] }
```

Se o OpenSSL já for uma dependência transitiva, esta funcionalidade pode ser activada através da dependência correspondente, desde que a resolução de versões seja compatível.

Esta opção compila o código-fonte do OpenSSL em vez de depender de uma instalação prévia. Contudo, não elimina todos os requisitos de cross-compilation: ainda podes precisar de um compilador C, ferramentas de construção e uma toolchain compatível com Linux.

## Qual escolher?

| Objectivo                                | Recomendação                            |
| ---------------------------------------- | --------------------------------------- |
| Windows → Linux x86-64                   | WSL2 ou Docker                          |
| Windows → Raspberry Pi ARMv7             | Cross-compilation com toolchain ARM     |
| Windows → Raspberry Pi ARM64             | Cross-compilation com toolchain AArch64 |
| Evitar dependência do OpenSSL do sistema | Avaliar `vendored`                      |

Para o teu projecto `fogu_cli`, começaria pelo WSL2. É uma forma simples de resolver o problema do OpenSSL e de outras dependências nativas, sem teres de configurar manualmente `PKG_CONFIG_SYSROOT_DIR`, `PKG_CONFIG_PATH` e um wrapper de `pkg-config`.

Se o objectivo final for o Raspberry Pi, diz-me se estás a utilizar Linux de 32 ou 64 bits, porque nesse caso a configuração será diferente.