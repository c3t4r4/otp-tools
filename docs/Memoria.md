# Memória do Projeto

## Decisões técnicas e aprendizados

- **Protobuf sem `protoc`**: Utilizado `prost` com structs anotadas em `src/proto.rs` espelhando `proto/otpauth-migration.proto`, evitando dependência de `protoc` instalado no host.
- **QR Code em Rust puro**: Utilizado `image` (`png`, `jpeg`) + `rqrr` com composição alfa sobre fundo branco (`to_luma8_with_white_bg`), evitando a dependência nativa `libzbar` usada no projeto Python original.
- **Tratamento de `+` em Base64 na query string**: `url::Url::query_pairs()` decodifica `+` como espaço (`application/x-www-form-urlencoded`); em `src/migration.rs`, espaços no parâmetro `data` são normalizados de volta para `+` antes do decode Base64.
- **Proteção contra Terminal Escape Injection**: Todo texto vindo de QR Code (incluindo payloads inválidos) passa por `sanitize_terminal_text` em `src/model.rs` antes da exibição.
- **Limites de imagem**: `src/qrcode.rs` impõe limite de 20 MB no arquivo, 10.000 px por dimensão e 256 MB de alocação máxima no decodificador `image`.
- **GUI em `eframe`/`egui` + `rfd`**: Escolhido binário dedicado (`otp-tools-gui`) separado do CLI (`otp-tools`), com estado de lógica isolado em `GuiState` para testes unitários sem display.
- **Windows GUI sem console**: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` em `src/bin/otp-tools-gui.rs`.
- **Builds multiplataforma**: Compilação nativa via GitHub Actions (`macos-latest` ARM64, `windows-latest` x64, `ubuntu-latest` x64); no Linux, `eframe` + `rfd` requerem `libgtk-3-dev`, `libxcb-*`, `libxkbcommon-dev`, `libssl-dev` e `pkg-config`.
- **Bundle macOS `.app`**: `scripts/package-macos-app.sh` gera `OTP Tools.app` (`otp-tools-gui`, `com.otp-tools.gui`) com ícone nativo `assets/AppIcon.icns` (`Contents/Resources/AppIcon.icns` + `CFBundleIconFile=AppIcon`), `Info.plist` (`NSHighResolutionCapable=true`, `CFBundlePackageType=APPL`), valida com `plutil -lint` e aplica assinatura ad-hoc (`codesign --force --deep --sign -`), exigida no Apple Silicon para integridade do bundle.
- **Ícone runtime e QR Code por conta**: `image` com `features = ["png", "jpeg", "ico"]` decodifica `assets/AppIcon.ico` embutido (`include_bytes!`) sem crates externas adicionais; `qrcode` em `[dependencies]` gera `egui::ColorImage` por conta, cacheado em `HashMap<usize, TextureHandle>` e limpo junto com `zeroize` em `clear_all`/`on_exit`.
- **Nitidez de fonte e layout sem estouro no `egui`**:
  - **Fontes variáveis (`fvar`) vs estáticas no `ab_glyph`**: Fontes do sistema como `SFNS.ttf` e `SFNSMono.ttf` possuem tabela variável `fvar` com ajustes de largura em `HVAR` que o `ab_glyph` do `egui` não processa, provocando colisão/sobreposição entre letras; utilizar fontes **estáticas** (`HelveticaNeue.ttc`/`Arial.ttf` e `Menlo.ttc`/`Monaco.ttf`) resolve o kerning.
  - **Anti-aliasing de bordas arredondadas**: Manter `tessellation_options.feathering = true` evita bordas serrilhadas em pílulas, botões e cards arredondados.
  - **Cobertura de glifos em `HelveticaNeue.ttc`**: O caractere `"⇄"` (`U+21C4`) não está presente na face carregada de `HelveticaNeue.ttc` e renderiza como `□`; desenhar ícones geométricos na sidebar diretamente via `ui.painter().line_segment(...)` garante nitidez multiplataforma independente de fonte.
  - **Padding e centralização em `egui::TextEdit`**: `TextEdit::singleline` com `.margin(Margin::symmetric(16, 12))` + `.vertical_align(Align::Center)` + `.horizontal_align(Align::Center)` evita o efeito de texto colado no topo de `multiline`; descontar a largura da margem horizontal (`card_inner_w - 34.0`) em `desired_width` evita que a borda do input ultrapasse a margem interna do card.
  - **Sincronia entre margem de cards e `CARD_FRAME_CHROME_W`**: Ao alterar `card_frame` (`Margin::symmetric(24, 22)` = `48 px` horizontais + `2 px` de borda = `50.0`), atualizar `CARD_FRAME_CHROME_W` proporcionalmente preserva a ausência de estouro horizontal e o breakpoint responsivo (`>= 560 px`).
  - **Badges de alto contraste**: Usar preenchimento sólido vibrante (`success`, `secondary_badge`, `danger`) com texto `Color32::WHITE` em vez de texto colorido sobre fundo da mesma cor com baixa opacidade (`linear_multiply(0.18)`).
  - Nunca usar `desired_width(f32::INFINITY)` dentro de `ui.horizontal` / `ui.horizontal_top` no `egui`, pois expande a coluna esquerda para fora do card e corta a coluna direita; passar sempre largura finita calculada (`data_w - 26.0`) e controlar `item_spacing.x` via `ui.scope` para divisão exata `2/3` dados + `1/3` QR Code.
- **Painel "About" nativo no macOS `.app`**: `NSApplication` lê `NSHumanReadableCopyright` do `Contents/Info.plist` e os créditos ricos de `Contents/Resources/Credits.rtf` (evitar cor fixa `\cf0` no RTF para que o texto se adapte automaticamente ao Dark/Light Mode do macOS).
- **Captura de câmera multiplataforma sem `bindgen`**: Crates como `nokhwa`/`v4l2-sys-mit` exigem `libclang-dev` no Linux e costumam quebrar em cabeçalhos do kernel 6.x com `bindgen` antigo. A implementação direta em `src/camera.rs` sobre `libc` (Linux `V4L2` com `ioctl`/`mmap` e alinhamento correto da union de 208 bytes de `v4l2_format` em 64 bits), `objc` (macOS `AVFoundation` + `_NSConcreteStackBlock` declarado como símbolo de array `[ *const c_void; 32 ]` usando `addr_of!` para obter `__NSStackBlock__` e `_Block_copy`/`_Block_release` para promover o `AccessBlock` ao heap com segurança em `requestAccessForMediaType:`) e Win32 (`avicap32`/`user32`) compila 100% offline usando apenas dependências já presentes no `Cargo.lock` (MSRV `Rust 1.88+`, devido a `<[T]>::as_chunks` e `u32::is_multiple_of`).
- **Limites temporais e de buffer na câmera**: `scan_qr_from_camera_blocking` usa `Instant::checked_add`, `--timeout` no CLI é limitado ao intervalo `1..=3600` segundos, `run_qr_scan_loop`/`linux_camera` usam `Option<Instant>` no lugar de subtração direta e `did_output_sample_buffer` valida explicitamente `CVPixelBufferGetPixelFormatType == kCVPixelFormatType_32BGRA` antes de copiar pixels.
- **Lint `unexpected_cfgs` de macros `objc 0.2`**: As macros `msg_send!` / `sel_impl` expandem `#[cfg(feature = "cargo-clippy")]`; declarar `[lints.rust] unexpected_cfgs = { level = "warn", check-cfg = ['cfg(feature, values("cargo-clippy"))'] }` no `Cargo.toml` evita falsos positivos sem suprimir outros avisos.
- **Permissão de câmera e resolução 720p no macOS**: O `Contents/Info.plist` de `OTP Tools.app` exige `<key>NSCameraUsageDescription</key>` para que o TCC solicite acesso em vez de encerrar o app. Manter `PREVIEW_MAX_WIDTH = 1280` garante leitura nítida de QR Codes densos (`otpauth-migration://`) fotografados de telas de celular.
- **Ícone `.icns` no modo Lista do Finder (`IconServices`)**: Chunks PNG pequenos (`icp4` 16x16 e `icp5` 32x32) com perfil `iCCP` de ~2,6 KB embutido no PNG e ausência dos chunks Retina (`ic11`/`ic12`) ou legados (`is32`/`s8mk` e `il32`/`l8mk`) fazem o `IconServices` renderizar barras verdes/amarelas/magenta corrompidas no modo Lista do Finder. Gerar o `.icns` no formato nativo Apple (`TOC ` + `is32`/`s8mk` 16x16 PackBits RLE + `il32`/`l8mk` 32x32 PackBits RLE + `ic07`..`ic14` com chunk `sRGB` de 1 byte e `pHYs` de 72/144 DPI, sem `iCCP`) e atualizar o mtime do `.app` (`touch`) garante renderização nítida tanto no modo Lista quanto em Miniatura.
- **Compatibilidade com `rustc` / `clippy` `1.100.0+` no CI (`-D warnings`)**:
  - **`float_literal_f32_fallback` (`rust-lang/rust#154024`)**: Métodos genéricos como `egui::Stroke::new(width: impl Into<f32>, ...)` passaram a rejeitar literais float sem sufixo sob `-D warnings` porque `f32: From<f64>` não é satisfeito; explicitar sempre `<valor>_f32` (ex.: `1.0_f32`).
  - **`clippy::chunks_exact_to_as_chunks`**: Iterações sobre fatias com tamanho constante conhecido (`chunks_exact(4)` em quadros YUYV/RGBA) passam a exigir `.as_chunks::<4>().0` (estável desde o Rust 1.88.0).
  - **`dead_code` condicional por SO em `src/camera.rs`**: `FrameSink` e `run_qr_scan_loop` são usados apenas por `macos_camera` e `windows_camera` (pois `linux_camera` faz polling V4L2 direto), exigindo `#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]`.
  - **`function_casts_as_integer` e `clippy::fn_to_numeric_cast` no Windows**: Passar callbacks FFI para `LPARAM` (`isize`) em `SendMessageW` exige cast intermediário para ponteiro bruto (`frame_callback as *const () as isize`).

## Comandos descobertos

- `cargo test --offline` — executa toda a suíte unitária e de integração sem rede após `cargo fetch`.
- `cargo clippy --offline --all-targets -- -D warnings` — lint estrito.
- `cargo fmt --check` — verifica formatação `rustfmt`.
- `cargo build --release --offline --bins` — compila ambos os binários (`otp-tools` e `otp-tools-gui`) em release.
- `cargo run --bin otp-tools -- qr --camera` — aciona a câmera/webcam no CLI para ler um QR Code do celular.
- `cargo run --bin otp-tools-gui` — executa a interface gráfica desktop **OTP Tools**.
- `./scripts/package-macos-app.sh` — gera `dist/OTP Tools.app` a partir do build release local.

## Histórico

- `2026-11-19`: Registro inicial de decisões técnicas, armadilhas de decodificação e comandos úteis.
- `2026-11-19`: Registro das decisões de GUI (`eframe`/`egui` + `rfd`), `windows_subsystem` e matriz de build multiplataforma.
- `2026-11-19`: Registro do script e empacotamento macOS `.app` (`scripts/package-macos-app.sh`).
- `2026-11-19`: Registro da geração e versionamento de `assets/AppIcon.icns`.
- `2026-11-19`: Registro da renomeação para **OTP Tools**, feature `ico` em `image` para ícone runtime e cache seguro de texturas QR na GUI.
- `2026-11-19`: Registro dos aprendizados sobre nitidez (`SFNS` + `feathering = false`), divisão finita `2/3` + `1/3` no `egui` e `Credits.rtf` / `NSHumanReadableCopyright` no painel About do macOS.
- `2026-11-19`: Registro da armadilha de fontes variáveis `fvar` (`SFNS`) vs estáticas (`HelveticaNeue.ttc`/`Menlo.ttc`) no `ab_glyph`, restauração de `feathering = true` e contraste de badges.
- `2026-11-19`: Registro da arquitetura de captura de câmera nativa sem `bindgen`, configuração `[lints.rust]` para `objc 0.2`, `NSCameraUsageDescription` no macOS e comando `otp-tools qr --camera`.
- `2026-11-20`: Registro da causa raiz e correção da corrupção do ícone no modo Lista do Finder (`icp4`/`icp5` com `iCCP` vs formato nativo `TOC `, `is32`/`s8mk`, `il32`/`l8mk` e Retina `ic11`..`ic14`).
- `2026-11-20`: Registro da constante `CARD_FRAME_CHROME_W` vinculada à margem interna dos cards e padronização de espaçamento em `src/gui.rs`.
- `2026-11-20`: Registro da substituição vetorial de `U+21C4` no ícone **Migration** e do ajuste de margem/alinhamento em `TextEdit::singleline`.
- `2026-11-23`: Registro da compatibilidade com Rust 1.100.0 no CI (`_f32` em `Stroke::new` contra `float_literal_f32_fallback` e `.as_chunks::<4>().0` contra `clippy::chunks_exact_to_as_chunks`).
- `2026-11-23`: Registro da supressão `dead_code` específica de plataforma (`macos`/`windows`) em `FrameSink`/`run_qr_scan_loop` e do cast `frame_callback as *const () as isize` no Windows em `src/camera.rs`.
