# OTP Tools

Aplicação **CLI + GUI** 100% offline em Rust para ler QR Codes (`PNG`, `JPG`, `JPEG`), decodificar links `otpauth-migration://` do Google Authenticator, analisar links `otpauth://totp/` e `otpauth://hotp/`, e gerar um **QR Code individual** para cada conta (pronto para adicionar no seu aplicativo OTP preferido), com builds para **macOS ARM64**, **Windows x64** e **Linux x64**.

## Funcionalidades

- **Leitura de QR Code (`PNG`, `JPG`, `JPEG` ou Câmera/Webcam)**:
  - Se o QR Code contiver `otpauth://totp/` ou `otpauth://hotp/`, separa e exibe todos os campos (tipo, conta/label, emissor, secret Base32, algoritmo, dígitos, período/contador) e o link completo.
  - Se contiver `otpauth-migration://offline?data=...`, decodifica o payload Protobuf e exibe os dados separados e o link completo `otpauth://` de todas as contas resultantes.
  - Se não for nenhum desses formatos, informa claramente que é um **QR Code inválido** e exibe o conteúdo bruto encontrado (sanitizado contra sequências de escape de terminal).
  - Na câmera/webcam, basta apontar o QR Code da tela do celular; ao detectar e decodificar o código, a webcam é desligada automaticamente.
- **Interface Gráfica Desktop (`otp-tools-gui` / `OTP Tools.app`)** em `egui`/`eframe`:
  - Design moderno com cantos arredondados e temas **Escuro / Claro**, carregando `assets/AppIcon.ico` em runtime.
  - **QR Code gerado ao lado de cada conta** para escaneamento direto no seu app OTP preferido (Google Authenticator, Authy, 1Password, Apple Passwords, Bitwarden, Ente Auth etc.), oculto por padrão por privacidade com botão **Mostrar QR** individual ou **Mostrar Secrets** global.
  - **Drag & Drop** de imagens QR Code diretamente na janela + seletor nativo de arquivos (`rfd`) + leitura ao vivo pela **Câmera / Webcam** (`Usar Câmera` com viewfinder e detecção automática).
  - 4 abas: **Ler QR Code**, **Decodificar `otpauth-migration://`**, **Analisar `otpauth://`** e **Sobre**.
  - Cópia rápida (**Copiar Secret**, **Copiar Link**, **Copiar todos os links**) e botão **Limpar tela** (zeroiza memória, desativa câmera se ativa, limpa undo history e cache de texturas QR).
- **Interface CLI (`otp-tools`)**:
  - Subcomandos `qr` (arquivo ou `--camera`), `decode` e `parse` com tabelas coloridas (`comfy-table` + `owo-colors`), modo interativo (`cliclack`) sem argumentos e flag `--plain` para scripts/pipes.

## Requisitos

- Rust 1.88+ / Cargo (testado em `rustc 1.94.1` e compatível com `rustc 1.100.0`)
- No **Linux x64** (apenas para compilar a GUI a partir do código-fonte):
  ```bash
  sudo apt-get install -y libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config
  ```

## Instalação e build

```bash
cargo build --release --bins
```

Os executáveis serão gerados em:
- CLI: `target/release/otp-tools` (ou `.exe` no Windows)
- GUI: `target/release/otp-tools-gui` (ou `.exe` no Windows)

### Gerar aplicativo `.app` nativo no macOS

```bash
cargo build --release --bin otp-tools-gui
./scripts/package-macos-app.sh
```

O aplicativo será gerado em `dist/OTP Tools.app` (com ícone nativo `AppIcon.icns`, `Info.plist` Retina/HiDPI e assinatura ad-hoc para Apple Silicon). Você pode abri-lo diretamente no Finder ou arrastá-lo para `/Applications`.
> **Nota (macOS Gatekeeper)**: builds baixados da internet sem notarização Apple podem exigir clique direito -> **Abrir** na primeira execução.

## Builds multiplataforma (macOS ARM, Windows x64, Linux x64)

O repositório inclui pipelines GitHub Actions em [.github/workflows/ci.yml](.github/workflows/ci.yml) e [.github/workflows/release.yml](.github/workflows/release.yml):
- **macOS ARM64** (`aarch64-apple-darwin`) -> `otp-tools-macos-arm64.tar.gz` (`OTP Tools.app` + CLI `otp-tools` + `README.md` + `LICENSE`)
- **Windows x64** (`x86_64-pc-windows-msvc`) -> `otp-tools-windows-x64.zip` (`otp-tools.exe` + `otp-tools-gui.exe` + `README.md` + `LICENSE`)
- **Linux x64** (`x86_64-unknown-linux-gnu`) -> `otp-tools-linux-x64.tar.gz` (`otp-tools` + `otp-tools-gui` + `README.md` + `LICENSE`)

Ao criar uma tag `v*` (ex.: `git tag v0.1.0 && git push origin v0.1.0`), o workflow compila ambos os binários em cada sistema operacional, gera `SHA256SUMS.txt` e publica automaticamente na GitHub Release.

## Uso

### Abrir interface gráfica (GUI)

```bash
cargo run --bin otp-tools-gui
# ou no macOS após empacotar:
open "dist/OTP Tools.app"
```

### Modo interativo no terminal (CLI sem argumentos)

```bash
cargo run --bin otp-tools
```

### Ler QR Code de imagem (PNG, JPG ou JPEG) ou pela câmera do PC

```bash
cargo run --bin otp-tools -- qr ./qrcode.png
# ou
cargo run --bin otp-tools -- qr --file ./qrcode.jpg
# ou usando a câmera/webcam do computador (limite padrão de 30s):
cargo run --bin otp-tools -- qr --camera
cargo run --bin otp-tools -- qr --camera --timeout 60
# apenas links puros:
cargo run --bin otp-tools -- qr ./qrcode.png --plain
```

### Decodificar link `otpauth-migration://`

```bash
cargo run --bin otp-tools -- decode --migration "otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC"
# apenas links puros:
cargo run --bin otp-tools -- decode "otpauth-migration://offline?data=..." --plain
```

### Analisar link `otpauth://` (TOTP ou HOTP)

```bash
cargo run --bin otp-tools -- parse "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP"
```

## Configuração

Nenhuma variável de ambiente é necessária. A ferramenta opera 100% offline e não grava segredos em disco.

## Testes e qualidade

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
shellcheck scripts/package-macos-app.sh
```

## Autor e repositório

- **Criado por**: Glauco G. Cetara (<glaucogcetara@gmail.com>)
- **GitHub**: https://github.com/c3t4r4/otp-tools.git

## Documentação de governança

- [AGENTS.md](./AGENTS.md)
- [Regras de Negócio](./docs/RegrasNegocio.md)
- [Arquitetura](./docs/Arquitetura.md) ([versão interativa](./docs/arquitetura.html))
- [Organograma](./docs/Organograma.md)
- [Frontend / GUI](./docs/Frontend.md)
- [Infraestrutura e Release](./docs/Infraestrutura.md)
- [RAG](./docs/RAG.md)
- [Progresso](./docs/Progresso.md)
- [Memória](./docs/Memoria.md)
- [Harness](./docs/Harness.md)

## Licença

[MIT License](./LICENSE) — Copyright (c) 2026 Glauco G. Cetara

## Histórico

- `2026-11-23`: Atualização do requisito mínimo de compilação (MSRV) para Rust 1.88+ (`<[T]>::as_chunks`) e compatibilidade com lints do Rust 1.100.0.
