# Organograma

## Arquitetura de módulos e fluxo de processamento — OTP Tools (CLI + GUI)

```mermaid
flowchart TD
    User["Usuário (Terminal / Desktop)"] --> MainCli["src/main.rs (otp-tools)"]
    User --> MainGui["src/bin/otp-tools-gui.rs"]

    MainCli --> Cli["src/cli.rs (clap + cliclack)"]
    MainGui --> Gui["src/gui.rs (eframe/egui + rfd + qrcode)"]

    Cli -->|"qr <arquivo>"| Qr["src/qrcode.rs (image + rqrr)"]
    Cli -->|"qr --camera"| Cam["src/camera.rs (AVFoundation / V4L2 / Win32)"]
    Cli -->|"decode <uri>"| Mig["src/migration.rs (base64 + prost + base32)"]
    Cli -->|"parse <uri>"| Otp["src/otpauth.rs (url + percent-encoding)"]

    Gui -->|"Drag & Drop / FileDialog"| Qr
    Gui -->|"Usar Câmera (Viewfinder)"| Cam
    Cam -->|"Preview RGBA8"| Gui
    Cam -->|"classify_qr_from_rgba_frame"| Qr
    Gui -->|"Aba Migration"| Mig
    Gui -->|"Aba OTPAuth"| Otp

    Qr -->|"otpauth://totp|hotp"| Otp
    Qr -->|"otpauth-migration://"| Mig
    Qr -->|"outro conteúdo"| Invalid["QrClassification::Invalid"]

    Mig --> Proto["src/proto.rs (Payload / OtpParameters)"]
    Mig --> Model["src/model.rs (OtpAccount / sanitize)"]
    Otp --> Model
    Invalid --> Model

    Model --> Ui["src/ui.rs (comfy-table + owo-colors)"]
    Model --> Gui
    Ui --> User
    Gui -->|"Cards + QR Code por conta"| User
```

## Histórico

- `2026-11-19`: Criação inicial do organograma de módulos e fluxo de dados.
- `2026-11-19`: Atualização do diagrama incluindo o binário GUI (`otpauth-migration-gui`) e o módulo `src/gui.rs`.
- `2026-11-19`: Atualização dos nomes para **OTP Tools** (`otp-tools`, `otp-tools-gui`) e saída com geração de QR Code por conta na GUI.
- `2026-11-19`: Inclusão de `src/camera.rs` conectado ao fluxo da GUI, CLI e `src/qrcode.rs`.
