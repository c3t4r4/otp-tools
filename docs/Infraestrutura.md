# Infraestrutura, Build e Release Multiplataforma

## Visão geral

O **OTP Tools** é uma aplicação desktop/CLI offline sem servidores, bancos de dados ou containers em produção. A infraestrutura consiste em:
- Compilação local nativa (macOS ARM64 `aarch64-apple-darwin`) + empacotamento `OTP Tools.app`.
- Pipelines automatizados no **GitHub Actions** para validação contínua (CI) e empacotamento de releases multiplataforma (**macOS ARM64**, **Windows x64** e **Linux x64**).

## Matriz de targets suportados

| Plataforma  | Target Rust                   | Runner GitHub Actions | Artefato de Release            | Conteúdo incluído                                  |
| ----------- | ----------------------------- | --------------------- | ------------------------------ | -------------------------------------------------- |
| macOS ARM   | `aarch64-apple-darwin`        | `macos-latest`        | `otp-tools-macos-arm64.tar.gz` | `OTP Tools.app` (GUI), `otp-tools` (CLI), `README.md`, `LICENSE` |
| Windows x64 | `x86_64-pc-windows-msvc`      | `windows-latest`      | `otp-tools-windows-x64.zip`    | `otp-tools.exe`, `otp-tools-gui.exe`, `README.md`, `LICENSE`     |
| Linux x64   | `x86_64-unknown-linux-gnu`    | `ubuntu-latest`       | `otp-tools-linux-x64.tar.gz`   | `otp-tools`, `otp-tools-gui`, `README.md`, `LICENSE`             |

## Workflows GitHub Actions

- **`.github/workflows/ci.yml`**:
  - Disparo: `push` na branch `main` e tags `v*`, e `pull_request`.
  - Permissões: `contents: read` (e `actions/checkout` com `persist-credentials: false`).
  - Etapas nas 3 plataformas: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` e `cargo build --locked --release --bins`.
- **`.github/workflows/release.yml`**:
  - Disparo: tags `v*` (publica GitHub Release automaticamente) ou `workflow_dispatch` (gera artefatos para download manual).
  - Permissões: `contents: read` nos jobs de build (`persist-credentials: false` no checkout); `contents: write` restrito ao job `publish`.
  - Executa `cargo test --locked` antes do build `--release --locked`, empacota CLI + GUI + `README.md` + `LICENSE` em `.tar.gz`/`.zip`, gera `SHA256SUMS.txt` de forma determinística e publica os arquivos na Release.

## Dependências de sistema para build no Linux x64

Para compilar `eframe` (OpenGL/X11/Wayland) e suporte nativo de diálogo (`rfd` utiliza `xdg-portal` por padrão no Linux) no Ubuntu/Debian:
- `libgtk-3-dev`, `libxcb-render0-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libxkbcommon-dev`, `libssl-dev`, `pkg-config`.
- A captura de câmera no Linux (`V4L2`) utiliza apenas chamadas de sistema (`ioctl`/`mmap`) via crate `libc`, sem requerer `libv4l-dev` nem `libclang-dev` em tempo de compilação.

## Configuração específica de plataforma

- **macOS ARM64 (`.app` bundle)**:
  - O script `scripts/package-macos-app.sh` empacota `otp-tools-gui` como `OTP Tools.app` (`Contents/MacOS/otp-tools-gui`, `Contents/Resources/AppIcon.icns` copiado de `assets/AppIcon.icns` com chunks `TOC `, `is32`/`s8mk`, `il32`/`l8mk`, `ic07`..`ic14` compatíveis com o modo Lista e Retina do Finder sem `iCCP` inflado em miniatura, `Contents/Resources/Credits.rtf` com autor, e-mail, GitHub e licença MIT adaptável ao Dark/Light Mode, `Contents/Info.plist` com `CFBundleIdentifier=com.otp-tools.gui`, `CFBundleIconFile=AppIcon`, `CFBundlePackageType=APPL`, `NSHighResolutionCapable=true`, `LSMinimumSystemVersion=12.0`, `NSCameraUsageDescription` exigida pelo macOS TCC para acesso à webcam via `AVFoundation` e `NSHumanReadableCopyright`), valida o `Info.plist` com `plutil -lint`, aplica assinatura ad-hoc (`codesign --force --deep --sign -` + `codesign --verify --deep --strict`) e atualiza o timestamp do bundle (`touch`) para invalidar o cache de ícone no Finder.
  - Por usar assinatura ad-hoc (sem notarização Apple Developer), o macOS Gatekeeper pode exigir abertura via clique direito -> **Abrir** ou remoção do atributo de quarentena em downloads externos.
- **Windows x64**: utiliza `avicap32` + `user32` (Win32 SDK padrão, sem dependências externas de runtime) para leitura de webcam e `src/bin/otp-tools-gui.rs` usa `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` para abrir a GUI em release sem console auxiliar. Além disso, `src/gui.rs` embute `assets/AppIcon.ico` via `include_bytes!` para o ícone runtime.

## Variáveis de ambiente

Nenhuma variável de ambiente ou segredo externo é necessário para build ou execução (o job `publish` utiliza apenas o `GITHUB_TOKEN` efêmero padrão do GitHub Actions).

## Histórico

- `2026-11-19`: Criação inicial da documentação de infraestrutura e pipelines multiplataforma (`ci.yml` e `release.yml` para macOS ARM, Windows x64 e Linux x64).
- `2026-11-19`: Adição do empacotamento macOS `.app` (`scripts/package-macos-app.sh`, `Info.plist`, assinatura ad-hoc) e atualização do job `macos-arm64` em `release.yml`.
- `2026-11-19`: Inclusão do ícone customizado `assets/AppIcon.icns` (`CFBundleIconFile=AppIcon`) em `Contents/Resources/AppIcon.icns`.
- `2026-11-19`: Renomeação dos binários, artefatos e bundle para **OTP Tools** (`otp-tools`, `otp-tools-gui`, `OTP Tools.app`) e inclusão de `LICENSE` e ícone runtime `AppIcon.ico`.
- `2026-11-19`: Adição de `NSHumanReadableCopyright` ao `Info.plist` e geração de `Contents/Resources/Credits.rtf` para popular a janela nativa About do macOS `.app`.
- `2026-11-19`: Inclusão de `NSCameraUsageDescription` em `Contents/Info.plist` (`scripts/package-macos-app.sh`) e documentação dos backends nativos de câmera (`AVFoundation`, `V4L2` via `libc` e Win32 `avicap32`).
- `2026-11-20`: Regeneração de `assets/AppIcon.icns` com chunks `TOC `, `is32`/`s8mk`, `il32`/`l8mk` e `ic07`..`ic14` (sem `icp4`/`icp5` ou `iCCP`) para corrigir o modo Lista do Finder e adição de `touch` no bundle em `scripts/package-macos-app.sh`.
