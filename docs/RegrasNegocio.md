# Regras de Negócio

Memória viva das regras de negócio do **OTP Tools** (`otp-tools`).

## Entidades

- `E-OTPACCOUNT`: Conta OTP extraída ou decodificada (`otp_type`, `label`, `issuer`, `secret`, `algorithm`, `digits`, `period`, `counter`, `uri`).
- `E-QRCLASSIFICATION`: Resultado da classificação de um QR Code (`OtpAuth`, `Migration` ou `Invalid`).

## Regras

### Domínio QR Code (`QR`)

- **`RN-QR-001`** (`confirmada`): O leitor de QR Code deve aceitar apenas imagens nos formatos `PNG`, `JPG` e `JPEG`, validando tanto a extensão quanto o formato real (magic bytes) do arquivo.
  - Rastreabilidade: `src/qrcode.rs`, `tests/integration_tests.rs` (`qr_png_with_totp_uri`, `qr_jpg_with_hotp_uri`, `qr_jpeg_with_multi_account_migration_uri`, `qr_rejects_unsupported_extension_and_missing_file`, `qr_rejects_mismatched_magic_bytes_and_tiny_or_corrupt_png`).
- **`RN-QR-002`** (`confirmada`): Se o QR Code contiver um link `otpauth://totp/...` ou `otpauth://hotp/...` válido, o sistema deve exibir os dados separados por campo e o link completo.
  - Rastreabilidade: `src/qrcode.rs`, `src/otpauth.rs`, `src/ui.rs`, `tests/integration_tests.rs`.
- **`RN-QR-003`** (`confirmada`): Se o QR Code contiver um link `otpauth-migration://offline?data=...` válido, o sistema deve decodificar todo o payload e exibir os dados separados e o link completo `otpauth://` de todas as contas resultantes.
  - Rastreabilidade: `src/qrcode.rs`, `src/migration.rs`, `src/ui.rs`, `tests/integration_tests.rs`.
- **`RN-QR-004`** (`confirmada`): Se o QR Code não corresponder a `otpauth://totp/`, `otpauth://hotp/` nem `otpauth-migration://` válido, o sistema deve informar que é um **QR Code inválido** e exibir o conteúdo bruto encontrado (sanitizado contra caracteres de controle de terminal).
  - Rastreabilidade: `src/qrcode.rs`, `src/model.rs`, `src/ui.rs`, `tests/integration_tests.rs` (`qr_png_with_invalid_content_shows_sanitized_raw_payload`, `cli_plain_and_errors_never_emit_raw_ansi_escapes`).
- **`RN-QR-005`** (`⚠ inferida`): Imagens acima de 20 MB, com dimensão 0, dimensão superior a 10.000 px ou orçamento de buffer estimado superior a 256 MB devem ser rejeitadas antes da decodificação para evitar esgotamento de memória.
  - Rastreabilidade: `src/qrcode.rs` (`read_bounded_image_bytes`, `validate_image_dimensions`).
- **`RN-QR-006`** (`confirmada`): Na leitura via câmera/webcam do computador, os quadros RGBA8 capturados localmente são validados contra limites de dimensão (`validate_image_dimensions`) e zeroizados ao serem descartados (`CameraFrame::drop`), aplicando as mesmas regras de classificação (`RN-QR-002`..`004`) e desligando a câmera automaticamente assim que um QR Code é detectado.
  - Rastreabilidade: `src/qrcode.rs` (`classify_qr_from_rgba_frame`), `src/camera.rs` (`CameraSession`, `CameraFrame`, `scan_qr_from_camera_blocking`), `tests/integration_tests.rs` (`camera_frames_classify_totp_migration_and_invalid_and_feed_gui_state`).

### Domínio Migration (`MIG`)

- **`RN-MIG-001`** (`confirmada`): Um link de migração válido deve ter esquema `otpauth-migration`, host `offline`, ao menos um parâmetro de query `data` não vazio e ao menos uma conta válida com tipo (`TOTP`/`HOTP`) e `secret` não vazio.
  - Rastreabilidade: `src/migration.rs` (`validate_migration_uri`, `decode_migration_uri`, `otp_parameters_to_account`).
- **`RN-MIG-002`** (`confirmada`): Cada parâmetro `data` é decodificado em Base64 (tratando espaços provenientes de `+` não escapado) e desserializado como Protobuf `Payload`; cada `OtpParameters.secret` é codificado em Base32 sem padding (`=`).
  - Rastreabilidade: `src/migration.rs` (`decode_base64_payloads`, `encode_secret`, `decode_migration_uri`), `src/proto.rs`.
- **`RN-MIG-003`** (`⚠ inferida`): Contas `HOTP` decodificadas de `otpauth-migration://` incluem o parâmetro `counter` na URI `otpauth://` resultante e nos dados exibidos.
  - Rastreabilidade: `src/migration.rs` (`build_url_params`, `otp_parameters_to_account`).

### Domínio OTPAuth (`OTP`)

- **`RN-OTP-001`** (`confirmada`): Uma URI `otpauth://` válida deve ter tipo `totp` ou `hotp`, não conter caracteres de controle crus e conter o parâmetro obrigatório `secret` não vazio.
  - Rastreabilidade: `src/otpauth.rs` (`parse_otpauth_uri`).
- **`RN-OTP-002`** (`⚠ inferida`): Quando o parâmetro `issuer` não estiver presente na query string, mas o `label` estiver no formato `Issuer:conta`, o emissor é inferido do prefixo antes de `:`.
  - Rastreabilidade: `src/otpauth.rs` (`parse_otpauth_uri`).

### Domínio CLI (`CLI`)

- **`RN-CLI-001`** (`confirmada`): Quando executado sem subcomandos, o binário abre o menu interativo com `cliclack` (incluindo leitura de imagem, leitura via câmera/webcam, `decode` e `parse`); quando executado com `qr` (arquivo ou `--camera [--timeout <S>]`), `decode` ou `parse`, executa diretamente o subcomando solicitado.
  - Rastreabilidade: `src/cli.rs` (`Commands::Qr`, `handle_camera_qr`, `InteractiveAction::ScanCameraQr`), `tests/integration_tests.rs` (`cli_qr_camera_flags_and_timeout_validation`).
- **`RN-CLI-002`** (`⚠ inferida`): A flag `--plain` imprime os links `otpauth://` resultantes em linha única sanitizada (um por linha) em `stdout`; para QR Code inválido no subcomando `qr --plain`, emite aviso em `stderr` e imprime o conteúdo bruto sanitizado em linha única em `stdout`.
  - Rastreabilidade: `src/cli.rs` (`handle_decode`, `handle_parse`, `print_classification_plain`), `tests/integration_tests.rs` (`cli_decode_plain_matches_python_output`, `cli_plain_and_errors_never_emit_raw_ansi_escapes`).

### Domínio GUI (`GUI`)

Telas: `T-01` (Ler QR Code), `T-02` (Decodificar `otpauth-migration://`), `T-03` (Analisar `otpauth://`), `T-04` (Sobre).

- **`RN-GUI-001`** (`confirmada`): A interface gráfica é entregue em um binário dedicado (`otp-tools-gui`), mantendo o binário CLI (`otp-tools`) independente no mesmo projeto (**OTP Tools**).
  - Rastreabilidade: `Cargo.toml`, `src/bin/otp-tools-gui.rs`, `src/gui.rs`.
- **`RN-GUI-002`** (`confirmada`): Na tela `T-01`, o usuário pode selecionar uma imagem (`PNG`, `JPG`, `JPEG`) via seletor nativo, arrastar e soltar (drag-and-drop) sobre a janela ou acionar a **câmera/webcam** do PC (**Usar Câmera** com viewfinder ao vivo); o conteúdo segue as mesmas regras de classificação do domínio QR (`RN-QR-001`..`006`), desligando a câmera automaticamente após a leitura ou ao parar/trocar de aba/limpar/fechar.
  - Rastreabilidade: `src/gui.rs` (`process_qr_path`, `process_camera_classifications`, `start_camera`, `stop_camera`, `render_camera_viewfinder`, `tests::gui_state_handles_missing_qr_file_with_error`, `tests::gui_state_processes_camera_classifications_and_errors`), `tests/integration_tests.rs` (`gui_state_processes_valid_and_invalid_qr_images`, `camera_frames_classify_totp_migration_and_invalid_and_feed_gui_state`).
- **`RN-GUI-003`** (`⚠ inferida`): Na GUI, segredos Base32 (incluindo o parâmetro `secret=` no campo somente-leitura **Link completo** e os campos de entrada) são exibidos mascarados por padrão (ocultação integral para segredos <= 8 caracteres; prefixo de 4 caracteres para maiores) até que o usuário ative explicitamente **Mostrar Secrets**, enquanto botões dedicados permitem copiar Secret, Link ou Todos os links para a área de transferência.
  - Rastreabilidade: `src/gui.rs` (`mask_secret`, `mask_otpauth_uri_secret`, `GuiState::reveal_secrets`, `tests::mask_secret_handles_short_and_long_secrets`, `tests::mask_otpauth_uri_secret_hides_secret_param_and_preserves_rest`).
- **`RN-GUI-004`** (`⚠ inferida`): O botão **Limpar tela** (assim como os botões **Limpar** das abas e o encerramento da janela) zeroiza os buffers sensíveis, remove o histórico de undo (`TextEditState`), limpa o cache de texturas de QR Code, apaga os resultados exibidos e retorna `reveal_secrets` para `false`.
  - Rastreabilidade: `src/gui.rs` (`GuiState::clear_all`, `clear_input_undo_history`, `tests::gui_state_decodes_migration_and_clears_sensitive_state`, `tests::clear_input_undo_history_removes_stored_text_edit_states`).
- **`RN-GUI-005`** (`confirmada`): O projeto oferece builds nativas de CLI (`otp-tools`) e GUI (`otp-tools-gui`) para **macOS ARM** (`aarch64-apple-darwin`, empacotado como bundle nativo `OTP Tools.app` + binário CLI), **Windows x64** (`x86_64-pc-windows-msvc`) e **Linux x64** (`x86_64-unknown-linux-gnu`).
  - Rastreabilidade: `scripts/package-macos-app.sh`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `src/bin/otp-tools-gui.rs`.
- **`RN-GUI-006`** (`confirmada`): Cada conta exibida na GUI ocupa um card responsivo dividido em 3 partes (**2/3** para os dados estruturados em duas subcolunas de tiles + link completo + botões de cópia e **1/3** para o **QR Code** gerado dinamicamente a partir do link `otpauth://...`, empilhando verticalmente abaixo de `560 px` internos sem estouro); por privacidade, o QR Code inicia oculto quando `reveal_secrets == false` e pode ser revelado individualmente (botão **Mostrar QR** por conta) ou globalmente (**Mostrar Secrets**).
  - Rastreabilidade: `src/gui.rs` (`render_account_card`, `render_account_data_section`, `render_account_qr_section`, `generate_qr_color_image`, `GuiState::toggle_qr_visibility`, `tests::generate_qr_color_image_produces_square_bw_image`, `tests::gui_state_decodes_migration_and_clears_sensitive_state`), `tests/integration_tests.rs` (`gui_generated_qr_roundtrips_back_to_same_otpauth_account`).
- **`RN-GUI-007`** (`confirmada`): A GUI inclui a aba **Sobre (`T-04`)** e o painel nativo **About OTP Tools** do bundle macOS `.app` (`Credits.rtf` + `NSHumanReadableCopyright`) exibindo o nome **OTP Tools**, versão, criador (**Glauco G. Cetara**), e-mail (`glaucogcetara@gmail.com`), link clicável do repositório GitHub (`https://github.com/c3t4r4/otp-tools.git`) e **Licença MIT**.
  - Rastreabilidade: `src/gui.rs` (`GuiTab::About`, `APP_DISPLAY_NAME`, `APP_AUTHOR`, `APP_EMAIL`, `APP_REPO_URL`, `APP_LICENSE`), `scripts/package-macos-app.sh` (`Credits.rtf`, `NSHumanReadableCopyright`), `LICENSE`, `tests/integration_tests.rs` (`gui_about_tab_and_metadata_match_otp_tools_requirements`).

## Histórico

- `2026-11-19`: Criação inicial das regras de negócio (`RN-QR-001`..`005`, `RN-MIG-001`..`003`, `RN-OTP-001`..`002`, `RN-CLI-001`..`002`) na reescrita do projeto em Rust e revisão de status `confirmada`/`⚠ inferida` após validação.
- `2026-11-19`: Adição das regras de GUI e builds multiplataforma (`RN-GUI-001`..`005`, telas `T-01`..`T-03`).
- `2026-11-19`: Atualização de `RN-GUI-005` para exigir empacotamento nativo `.app` (`OTPAuth Migration.app`) no macOS ARM64.
- `2026-11-19`: Renomeação para **OTP Tools** (`otp-tools`, `otp-tools-gui`, `OTP Tools.app`), adição das regras `RN-GUI-006` (QR Code por conta) e `RN-GUI-007` (tela Sobre `T-04`).
- `2026-11-19`: Atualização de `RN-GUI-006` (layout responsivo em 3 partes: 2/3 dados e 1/3 QR Code) e `RN-GUI-007` (painel nativo About macOS via `Credits.rtf` e `NSHumanReadableCopyright`).
- `2026-11-19`: Adição de `RN-QR-006` e atualização de `RN-CLI-001` e `RN-GUI-002` para cobrir leitura de QR Code ao vivo via câmera/webcam com desligamento automático e zeroização de quadros.
