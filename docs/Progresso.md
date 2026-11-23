# Progresso

| Data       | Tarefa                                                                                  | Status    |
| ---------- | --------------------------------------------------------------------------------------- | --------- |
| 2026-11-19 | Reescrita do `otpauth-migration-decoder` em Rust com CLI (`clap`/`cliclack`/`comfy-table`/`owo-colors`), leitor de QR Code (`PNG`/`JPG`/`JPEG`), suporte a `otpauth://totp/`, `otpauth://hotp/`, `otpauth-migration://` e exibição sanitizada de QR Codes inválidos | concluída |
| 2026-11-19 | Implementação da GUI desktop (`otpauth-migration-gui` em `eframe`/`egui` + `rfd`), build local macOS ARM64 e workflows GitHub Actions (`ci.yml`, `release.yml`) para macOS ARM64, Windows x64 e Linux x64 | concluída |
| 2026-11-19 | Empacotamento nativo macOS `.app` (`dist/OTPAuth Migration.app` via `scripts/package-macos-app.sh` com `Info.plist`, validação `plutil` e assinatura ad-hoc `codesign`) integrado ao workflow `release.yml` | concluída |
| 2026-11-19 | Criação do ícone customizado `assets/AppIcon.png` / `assets/AppIcon.icns` (multi-resolução 16px–1024px) e integração em `OTPAuth Migration.app/Contents/Resources/AppIcon.icns` (`CFBundleIconFile=AppIcon`) | concluída |
| 2026-11-19 | Renomeação completa para **OTP Tools** (`otp-tools`, `otp-tools-gui`, `OTP Tools.app`), licença MIT (`LICENSE`), tema arredondado Dark/Light na GUI, ícone runtime (`assets/AppIcon.ico`), QR Code gerado por conta com privacidade e tela Sobre (`T-04`) | concluída |
| 2026-11-19 | Redesign Apple Liquid Glass na GUI (`SFNS`/`SFNSMono` sem `feathering`, barra de abas em cápsula), layout responsivo em 3 partes (`2/3` dados em 2 subcolunas de tiles + `1/3` QR Code sem estouro) e créditos nativos (`Credits.rtf` + `NSHumanReadableCopyright`) no bundle `OTP Tools.app` | concluída |
| 2026-11-19 | Redesign inspirado no CleanMyMac X (`src/gui.rs`): gradiente Mesh Royal Purple/Magenta, Sidebar lateral em pílula, cabeçalhos Hero com squircles, badges de alto contraste, troca de fontes variáveis `SFNS` por fontes estáticas (`HelveticaNeue.ttc`/`Menlo.ttc`) sem colisão de glifos e `feathering = true` | concluída |
| 2026-11-19 | Leitura de QR Code ao vivo pela câmera/webcam do PC (`src/camera.rs` sem `bindgen` para macOS AVFoundation, Linux V4L2 e Windows avicap32), viewfinder na aba `T-01` da GUI, flag `qr --camera` / menu interativo no CLI e `NSCameraUsageDescription` em `OTP Tools.app` | concluída |
| 2026-11-20 | Correção de `assets/AppIcon.icns` (substituição de `icp4`/`icp5` com `iCCP` por `is32`/`s8mk`, `il32`/`l8mk`, Retina `ic11`..`ic14` e `TOC `) para exibição nítida no modo Lista do Finder + `touch` no bundle e teste de regressão | concluída |
| 2026-11-20 | Refino visual em `src/gui.rs`: aumento de paddings e espaçamentos internos em toda a UI (separação vertical entre botões e textos explicativos, margens `24x22` nos cards com `CARD_FRAME_CHROME_W`) e troca do tom rosa dos botões para roxo-violeta azulado (`#6D43F6`/`#6348EE`) | concluída |
| 2026-11-20 | Correção do ícone do botão **Migration** na sidebar (setas bidirecionais desenhadas vetorialmente via `painter.line_segment`) e adição de padding (`16x12`) + centralização horizontal/vertical nos inputs de `T-02` e `T-03` | concluída |
| 2026-11-23 | Correção dos 21 erros de lint/compilação do Rust 1.100.0 no GitHub Actions CI (`float_literal_f32_fallback` com sufixo `_f32` em `Stroke::new` de `src/gui.rs` e `clippy::chunks_exact_to_as_chunks` com `.as_chunks::<4>().0` em `src/camera.rs` e `src/qrcode.rs`) | concluída |

## Histórico

- `2026-11-19`: Registro inicial da implementação completa em Rust e governança do projeto.
- `2026-11-19`: Registro da entrega da interface gráfica (`egui`/`eframe`) e empacotamento multiplataforma (macOS ARM, Windows x64, Linux x64).
- `2026-11-19`: Registro da criação do bundle nativo macOS `.app` (`OTPAuth Migration.app`).
- `2026-11-19`: Registro da criação e integração do ícone customizado `AppIcon.icns` no bundle macOS `.app`.
- `2026-11-19`: Registro da renomeação para **OTP Tools**, redesign da GUI, QR Code por conta, tela Sobre e licença MIT.
- `2026-11-19`: Registro do redesign Apple Liquid Glass, layout responsivo 2/3 + 1/3, nitidez tipográfica e painel About nativo do bundle macOS.
- `2026-11-19`: Registro do redesign Royal Purple/Magenta com Sidebar lateral, badges de alto contraste e fontes estáticas (`HelveticaNeue.ttc`/`Menlo.ttc`) com `feathering = true`.
- `2026-11-19`: Registro da funcionalidade de leitura de QR Code via câmera/webcam do computador na GUI, CLI e bundle macOS `.app`.
- `2026-11-20`: Registro da correção dos chunks de resolução pequena (`is32`/`s8mk`, `il32`/`l8mk`, `ic11`..`ic14`) em `assets/AppIcon.icns` para o modo Lista do Finder.
- `2026-11-20`: Registro do ajuste de espaçamentos/paddings na GUI e mudança dos botões primários para roxo-violeta azulado.
- `2026-11-20`: Registro da correção vetorial do ícone **Migration** e do padding/centralização nos campos de entrada (`T-02`/`T-03`).
- `2026-11-23`: Registro da correção dos 21 avisos promovidos a erro pelo Rust 1.100.0 (`float_literal_f32_fallback` e `clippy::chunks_exact_to_as_chunks`) nos workflows de CI.
