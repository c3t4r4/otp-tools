# Arquitetura

## Visão geral

O **OTP Tools** (`otp-tools`) é uma aplicação CLI e GUI escrita em Rust, estruturada como biblioteca compartilhada (`src/lib.rs`) + dois binários finos (`otp-tools` em `src/main.rs` e `otp-tools-gui` em `src/bin/otp-tools-gui.rs`), separando domínio/decodificação, leitura de imagem/QR Code, apresentação em terminal e interface gráfica desktop com geração de QR Code por conta.

## Camadas e módulos

| Módulo | Responsabilidade |
| --- | --- |
| `src/main.rs` | Ponto de entrada do binário CLI (`otp-tools`); faz parsing via `clap`, delega para `cli::run` e trata código de saída. |
| `src/bin/otp-tools-gui.rs` | Ponto de entrada do binário GUI (`otp-tools-gui`); inicia `gui::launch_gui` (com `windows_subsystem = "windows"` em release no Windows). |
| `src/cli.rs` | Definição dos subcomandos (`qr` com arquivo ou `--camera`, `decode`, `parse`), flag `--plain` e menu interativo (`cliclack`). |
| `src/gui.rs` | Interface gráfica desktop em `eframe`/`egui` + `rfd` + `qrcode` (tema arredondado Dark/Light, ícone runtime `assets/AppIcon.ico`, drag-and-drop, captura ao vivo pela câmera com viewfinder, QR Code por conta com privacidade e cache de texturas, tela Sobre `T-04`) e `GuiState` testável. |
| `src/camera.rs` | Captura multiplataforma de câmera/webcam em thread dedicada sem `bindgen` (`AVFoundation`/`CoreMedia`/`CoreVideo` no macOS, `V4L2` via `libc` no Linux e `avicap32` no Windows), conversores BGRA/YUYV/BGR24/MJPEG, `CameraFrame` zeroizado no `Drop` e parada automática ao detectar QR Code. |
| `src/ui.rs` | Renderização visual em terminal usando `comfy-table` e `owo-colors`, com sanitização de texto. |
| `src/qrcode.rs` | Validação de arquivo/formato (`PNG`, `JPG`, `JPEG`), limites de memória/dimensão, composição alfa sobre fundo branco, leitura de quadros RGBA8 (`classify_qr_from_rgba_frame`), detecção via `rqrr` e classificação de payload. |
| `src/migration.rs` | Validação de `otpauth-migration://offline?data=...`, decode Base64, decode Protobuf (`prost`), codificação Base32 sem padding (`data-encoding`) e construção de URLs `otpauth://`. |
| `src/otpauth.rs` | Parser e validador de URIs `otpauth://totp/` e `otpauth://hotp/`. |
| `src/proto.rs` | Mensagens e enums Protobuf (`prost::Message` / `prost::Enumeration`) equivalentes a `proto/otpauth-migration.proto`. |
| `src/model.rs` | Tipos de domínio (`OtpAccount`, `OtpType`, `Algorithm`, `QrClassification`) e `sanitize_terminal_text` / `sanitize_single_line_terminal_text`. |
| `src/error.rs` | Hierarquia de erros tipados (`AppError`) via `thiserror`. |

## Fluxo de dados

1. Entrada via CLI (`otp-tools`: subcomandos `qr` com arquivo ou `--camera`, `decode`, `parse` ou modo interativo `cliclack`) ou via GUI desktop (`otp-tools-gui`: drag-and-drop, seletor nativo `rfd::FileDialog`, botão **Usar Câmera** com viewfinder ao vivo ou abas `Migration`/`OTPAuth`/`Sobre`).
2. Para imagens e quadros de vídeo:
   - Arquivos: validação de caminho/tamanho/formato (leitura única sem TOCTOU) -> composição alfa (quando aplicável) -> escala de cinza -> `rqrr`.
   - Câmera (`src/camera.rs`): captura nativa em thread dedicada -> conversão para `CameraFrame` RGBA8 -> preview contínuo na GUI e detecção periódica via `classify_qr_from_rgba_frame` -> parada automática da webcam e zeroização de frames ao decodificar QR Code -> classificação (`OtpAuth` / `Migration` / `Invalid`).
3. Para `Migration`: validação da URI -> decode Base64 -> decode Protobuf `Payload` -> conversão para `Vec<OtpAccount>`.
4. Para `OtpAuth`: parsing da URI `otpauth://` -> `OtpAccount`.
5. Saída no terminal em tabelas coloridas (`comfy-table` + `owo-colors`) ou texto simples (`--plain`), e na GUI em cards arredondados (`egui`) com segredos e `secret=` da URI mascarados por padrão, QR Code por conta sob demanda (`qrcode` + cache de texturas limpo ao sair/limpar), campos somente leitura e botões de cópia rápida.

## Decisões arquiteturais (ADRs)

- **ADR-001 — `prost` sem `protoc` em build time**: As mensagens Protobuf são definidas diretamente com macros derive do `prost` em `src/proto.rs` (com `proto/otpauth-migration.proto` mantido como referência), eliminando a dependência externa de `protoc`.
- **ADR-002 — `rqrr` + `image` em Rust puro**: Substitui `pyzbar`/`libzbar` por decodificador QR puro Rust, simplificando compilação e distribuição multiplataforma.
- **ADR-003 — Sanitização de saída de terminal**: Todo texto proveniente de QR Codes passa por `sanitize_terminal_text` / `sanitize_single_line_terminal_text` para neutralizar sequências de controle/ANSI antes da exibição.
- **ADR-004 — GUI em `eframe`/`egui` + binário dedicado (`otp-tools-gui`)**: Mantém 100% Rust sem dependência de Node/WebView, compartilha diretamente toda a biblioteca de domínio e preserva o binário CLI enxuto.
- **ADR-005 — Builds nativas multiplataforma via GitHub Actions**: Matriz nativa (`macos-latest` ARM64 gerando `OTP Tools.app`, `windows-latest` x64, `ubuntu-latest` x64) em `.github/workflows/ci.yml` e `.github/workflows/release.yml`.
- **ADR-006 — Geração de QR Code por conta e ícone runtime via `AppIcon.ico`**: A crate `qrcode` gera `ColorImage` em memória (cacheada por geração de resultado e limpa com `zeroize` em `clear_all`/`on_exit`); `image` com feature `ico` decodifica `assets/AppIcon.ico` embutido para `ViewportBuilder::with_icon`.
- **ADR-007 — Captura nativa de câmera multiplataforma sem `bindgen`**: Em vez de depender de `nokhwa`/`v4l2-sys-mit` (que exigem `libclang-dev` e falham com cabeçalhos V4L2 modernos sem patches), `src/camera.rs` utiliza diretamente APIs nativas via `libc`/`objc` (`AVFoundation` no macOS com `NSCameraUsageDescription`, `V4L2` via `ioctl`/`mmap` no Linux e `avicap32`/`user32` no Windows), mantendo compilação `--offline` limpa, desligamento imediato do hardware após leitura e zeroização de quadros em `Drop`.

## Histórico

- `2026-11-19`: Documentação inicial da arquitetura Rust e ADRs (`ADR-001` a `ADR-003`).
- `2026-11-19`: Adição da camada GUI (`src/gui.rs`, `otpauth-migration-gui`), ADRs `ADR-004` e `ADR-005`, e regeneração de `docs/arquitetura.html`.
- `2026-11-19`: Renomeação para **OTP Tools** (`otp-tools`, `otp-tools-gui`), adição do `ADR-006` (QR Code por conta + ícone runtime `AppIcon.ico`) e regeneração de `docs/arquitetura.html`.
- `2026-11-19`: Inclusão do módulo `src/camera.rs`, fluxo de captura via câmera/webcam na GUI e CLI, adição do `ADR-007` e regeneração de `docs/arquitetura.html`.
