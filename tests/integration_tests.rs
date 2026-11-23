use std::fs;
use std::path::Path;
use std::process::Command;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::{ImageBuffer, ImageFormat, Luma, Rgba};
use prost::Message;
use qrcode::QrCode;
use tempfile::TempDir;

use otp_tools::camera::{CameraFrame, CameraSession, CameraUpdate};
use otp_tools::error::{AppError, EXAMPLE_MIGRATION};
use otp_tools::gui::{
    generate_qr_color_image, GuiResult, GuiState, GuiTab, APP_AUTHOR, APP_DISPLAY_NAME, APP_EMAIL,
    APP_LICENSE, APP_REPO_URL,
};
use otp_tools::model::{Algorithm, OtpType, QrClassification};
use otp_tools::proto::{OtpParameters, Payload, ProtoAlgorithm, ProtoDigitCount, ProtoOtpType};
use otp_tools::qrcode::{classify_qr_from_rgba_frame, read_and_classify_qr_image};
use otp_tools::ui::render_qr_classification;

fn save_qr_image(content: &str, path: &Path, format: ImageFormat) {
    let code = QrCode::new(content.as_bytes()).expect("gerar QR code de teste");
    let module_size = 8u32;
    let quiet_zone = 4u32;
    let width = (code.width() as u32 + 2 * quiet_zone) * module_size;
    let mut img: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, width, Luma([255]));

    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                let px = (x as u32 + quiet_zone) * module_size;
                let py = (y as u32 + quiet_zone) * module_size;
                for dy in 0..module_size {
                    for dx in 0..module_size {
                        img.put_pixel(px + dx, py + dy, Luma([0]));
                    }
                }
            }
        }
    }

    img.save_with_format(path, format)
        .expect("salvar imagem QR de teste");
}

#[test]
fn qr_png_with_totp_uri() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("totp.png");
    let uri = "otpauth://totp/Example%3Aalice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example&algorithm=SHA1&digits=6&period=30";
    save_qr_image(uri, &path, ImageFormat::Png);

    let results = read_and_classify_qr_image(&path).unwrap();
    assert_eq!(results.len(), 1);
    match &results[0] {
        QrClassification::OtpAuth(account) => {
            assert_eq!(account.otp_type, OtpType::Totp);
            assert_eq!(account.label, "Example:alice@google.com");
            assert_eq!(account.issuer.as_deref(), Some("Example"));
            assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
            assert_eq!(account.algorithm, Some(Algorithm::Sha1));
            assert_eq!(account.digits, Some(6));
            assert_eq!(account.period, Some(30));
            assert_eq!(account.uri, uri);
        }
        other => panic!("esperava OtpAuth, obtive {other:?}"),
    }
}

#[test]
fn qr_jpg_with_hotp_uri() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("hotp.jpg");
    let uri = "otpauth://hotp/Acme%3Abob%40acme.com?secret=JBSWY3DPEHPK3PXP&issuer=Acme&counter=99&digits=8";
    save_qr_image(uri, &path, ImageFormat::Jpeg);

    let results = read_and_classify_qr_image(&path).unwrap();
    assert_eq!(results.len(), 1);
    match &results[0] {
        QrClassification::OtpAuth(account) => {
            assert_eq!(account.otp_type, OtpType::Hotp);
            assert_eq!(account.label, "Acme:bob@acme.com");
            assert_eq!(account.issuer.as_deref(), Some("Acme"));
            assert_eq!(account.counter, Some(99));
            assert_eq!(account.digits, Some(8));
        }
        other => panic!("esperava OtpAuth HOTP, obtive {other:?}"),
    }
}

#[test]
fn qr_jpeg_with_multi_account_migration_uri() {
    let payload = Payload {
        otp_parameters: vec![
            OtpParameters {
                secret: b"Hello!\xde\xad\xbe\xef".to_vec(),
                name: "Example:alice@google.com".to_string(),
                issuer: "Example".to_string(),
                algorithm: ProtoAlgorithm::Sha1 as i32,
                digits: ProtoDigitCount::Six as i32,
                r#type: ProtoOtpType::Totp as i32,
                counter: 0,
            },
            OtpParameters {
                secret: b"Hello!".to_vec(),
                name: "Corp:bob@corp.io".to_string(),
                issuer: "Corp".to_string(),
                algorithm: ProtoAlgorithm::Sha256 as i32,
                digits: ProtoDigitCount::Eight as i32,
                r#type: ProtoOtpType::Hotp as i32,
                counter: 12,
            },
        ],
        version: 1,
        batch_size: 1,
        batch_index: 0,
        batch_id: 1,
    };

    let mut bytes = Vec::new();
    payload.encode(&mut bytes).unwrap();
    let encoded_data = STANDARD.encode(bytes);
    let migration_uri = format!("otpauth-migration://offline?data={encoded_data}");

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("migration.jpeg");
    save_qr_image(&migration_uri, &path, ImageFormat::Jpeg);

    let results = read_and_classify_qr_image(&path).unwrap();
    assert_eq!(results.len(), 1);
    match &results[0] {
        QrClassification::Migration { accounts, .. } => {
            assert_eq!(accounts.len(), 2);
            assert_eq!(accounts[0].otp_type, OtpType::Totp);
            assert_eq!(accounts[0].secret, "JBSWY3DPEHPK3PXP");
            assert_eq!(accounts[1].otp_type, OtpType::Hotp);
            assert_eq!(accounts[1].secret, "JBSWY3DPEE");
            assert_eq!(accounts[1].counter, Some(12));
        }
        other => panic!("esperava Migration, obtive {other:?}"),
    }
}

#[test]
fn qr_png_with_invalid_content_shows_sanitized_raw_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("invalid.png");
    let raw = "https://example.com/promo\x1b[31mINJECT";
    save_qr_image(raw, &path, ImageFormat::Png);

    let results = read_and_classify_qr_image(&path).unwrap();
    assert_eq!(results.len(), 1);
    match &results[0] {
        QrClassification::Invalid { raw_content, .. } => {
            assert_eq!(raw_content, raw);
        }
        other => panic!("esperava Invalid, obtive {other:?}"),
    }

    let rendered = render_qr_classification(&results[0], 1, 1);
    assert!(rendered.contains("QR Code inválido"));
    assert!(rendered.contains("https://example.com/promo"));
    assert!(!rendered.contains("\x1b[31mINJECT"));
}

#[test]
fn qr_rejects_unsupported_extension_and_missing_file() {
    let dir = TempDir::new().unwrap();
    let bad_ext = dir.path().join("sample.gif");
    fs::write(&bad_ext, b"GIF89a").unwrap();

    let err = read_and_classify_qr_image(&bad_ext).unwrap_err();
    assert!(matches!(err, AppError::UnsupportedImageFormat(_)));

    let missing = dir.path().join("does_not_exist.png");
    let err = read_and_classify_qr_image(&missing).unwrap_err();
    assert!(matches!(err, AppError::FileNotFound(_)));
}

#[test]
fn qr_rejects_image_without_qr_code() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("blank.png");
    let blank: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_pixel(128, 128, Luma([255]));
    blank.save_with_format(&path, ImageFormat::Png).unwrap();

    let err = read_and_classify_qr_image(&path).unwrap_err();
    assert!(matches!(err, AppError::NoQrCodeFound(_)));
}

#[test]
fn cli_decode_plain_matches_python_output() {
    let bin = env!("CARGO_BIN_EXE_otp-tools");
    let output = Command::new(bin)
        .args(["decode", "--migration", EXAMPLE_MIGRATION, "--plain"])
        .output()
        .expect("executar binário decode");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP"
    );
}

#[test]
fn cli_parse_shows_separated_fields_and_full_link() {
    let bin = env!("CARGO_BIN_EXE_otp-tools");
    let uri = "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP";
    let output = Command::new(bin)
        .args(["parse", uri])
        .output()
        .expect("executar binário parse");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TOTP"));
    assert!(stdout.contains("Example:alice@google.com"));
    assert!(stdout.contains("JBSWY3DPEHPK3PXP"));
    assert!(stdout.contains(uri));
}

#[test]
fn cli_qr_on_generated_png() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli_test.png");
    save_qr_image(EXAMPLE_MIGRATION, &path, ImageFormat::Png);

    let bin = env!("CARGO_BIN_EXE_otp-tools");
    let output = Command::new(bin)
        .args(["qr", path.to_str().unwrap()])
        .output()
        .expect("executar binário qr");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("otpauth-migration://"));
    assert!(stdout.contains("JBSWY3DPEHPK3PXP"));
    assert!(stdout.contains(
        "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP"
    ));
}

#[test]
fn qr_png_with_transparent_background_decodes_cleanly() {
    let uri = "otpauth://totp/Alpha%3Auser%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Alpha";
    let code = QrCode::new(uri.as_bytes()).expect("gerar QR code transparente");
    let module_size = 8u32;
    let quiet_zone = 4u32;
    let width = (code.width() as u32 + 2 * quiet_zone) * module_size;
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, width, Rgba([0, 0, 0, 0]));

    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                let px = (x as u32 + quiet_zone) * module_size;
                let py = (y as u32 + quiet_zone) * module_size;
                for dy in 0..module_size {
                    for dx in 0..module_size {
                        img.put_pixel(px + dx, py + dy, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }
    }

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("alpha.png");
    img.save_with_format(&path, ImageFormat::Png).unwrap();

    let results = read_and_classify_qr_image(&path).unwrap();
    assert_eq!(results.len(), 1);
    match &results[0] {
        QrClassification::OtpAuth(account) => {
            assert_eq!(account.label, "Alpha:user@example.com");
            assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
        }
        other => panic!("esperava OtpAuth, obtive {other:?}"),
    }
}

#[test]
fn qr_rejects_mismatched_magic_bytes_and_tiny_or_corrupt_png() {
    let dir = TempDir::new().unwrap();

    let fake_png = dir.path().join("fake.png");
    fs::write(&fake_png, b"GIF89a\x01\x00\x01\x00").unwrap();
    let err = read_and_classify_qr_image(&fake_png).unwrap_err();
    assert!(matches!(err, AppError::UnsupportedImageFormat(_)));

    let corrupt_png = dir.path().join("corrupt.png");
    fs::write(&corrupt_png, b"\x89PNG\r\n\x1a\nnot-a-valid-png").unwrap();
    let err = read_and_classify_qr_image(&corrupt_png).unwrap_err();
    assert!(matches!(err, AppError::ImageReadError(_)));

    let tiny_png = dir.path().join("tiny.png");
    let tiny: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Luma([255]));
    tiny.save_with_format(&tiny_png, ImageFormat::Png).unwrap();
    let err = read_and_classify_qr_image(&tiny_png).unwrap_err();
    assert!(matches!(err, AppError::NoQrCodeFound(_)));
}

#[test]
fn cli_plain_and_errors_never_emit_raw_ansi_escapes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ansi_otp.png");
    let malicious_otp = "otpauth://totp/Acme?secret=JBSWY3DPEHPK3PXP&note=\x1b[31mINJECT";
    save_qr_image(malicious_otp, &path, ImageFormat::Png);

    let bin = env!("CARGO_BIN_EXE_otp-tools");

    // qr --plain com QR malicioso (rejeitado como Invalid, sem ESC em stdout/stderr)
    let qr_plain = Command::new(bin)
        .args(["qr", path.to_str().unwrap(), "--plain"])
        .output()
        .expect("executar qr --plain");
    assert!(qr_plain.status.success());
    let stdout = String::from_utf8_lossy(&qr_plain.stdout);
    let stderr = String::from_utf8_lossy(&qr_plain.stderr);
    assert!(!stdout.contains('\x1b'));
    assert!(!stderr.contains('\x1b'));
    assert!(stderr.contains("QR Code inválido"));

    // qr normal com QR inválido
    let qr_rich = Command::new(bin)
        .args(["qr", path.to_str().unwrap()])
        .output()
        .expect("executar qr normal");
    assert!(qr_rich.status.success());
    let rich_stdout = String::from_utf8_lossy(&qr_rich.stdout);
    assert!(rich_stdout.contains("QR Code inválido"));
    assert!(!rich_stdout.contains("\x1b[31mINJECT"));

    // parse --plain com caracteres de controle deve falhar sem emitir ESC
    let parse_ctrl = Command::new(bin)
        .args(["parse", malicious_otp, "--plain"])
        .output()
        .expect("executar parse com controle");
    assert!(!parse_ctrl.status.success());
    assert!(!String::from_utf8_lossy(&parse_ctrl.stdout).contains('\x1b'));
    assert!(!String::from_utf8_lossy(&parse_ctrl.stderr).contains("\x1b[31mINJECT"));

    // parse com %1B em parâmetro inválido deve sanitizar stderr
    let parse_pct = Command::new(bin)
        .args([
            "parse",
            "otpauth://totp/Test?secret=JBSWY3DPEHPK3PXP&digits=%1B[31mFAKE",
        ])
        .output()
        .expect("executar parse com %1B");
    assert!(!parse_pct.status.success());
    let pct_stderr = String::from_utf8_lossy(&parse_pct.stderr);
    assert!(!pct_stderr.contains("\x1b[31mFAKE"));
    assert!(pct_stderr.contains("\\u{001B}"));
}

#[test]
fn gui_state_processes_valid_and_invalid_qr_images() {
    let dir = TempDir::new().unwrap();

    let migration_path = dir.path().join("gui_migration.png");
    save_qr_image(EXAMPLE_MIGRATION, &migration_path, ImageFormat::Png);

    let mut state = GuiState::default();
    state.process_qr_path(&migration_path);
    match &state.result {
        GuiResult::QrResults {
            source_label,
            items,
        } => {
            assert_eq!(source_label, "gui_migration.png");
            assert_eq!(items.len(), 1);
            assert!(matches!(items[0], QrClassification::Migration { .. }));
        }
        other => panic!("esperava QrResults Migration, obtive {other:?}"),
    }

    let invalid_path = dir.path().join("gui_invalid.jpg");
    save_qr_image(
        "https://example.org/not-otp",
        &invalid_path,
        ImageFormat::Jpeg,
    );
    state.process_qr_path(&invalid_path);
    match &state.result {
        GuiResult::QrResults {
            source_label,
            items,
        } => {
            assert_eq!(source_label, "gui_invalid.jpg");
            assert_eq!(items.len(), 1);
            match &items[0] {
                QrClassification::Invalid { raw_content, .. } => {
                    assert_eq!(raw_content, "https://example.org/not-otp");
                }
                other => panic!("esperava Invalid, obtive {other:?}"),
            }
        }
        other => panic!("esperava QrResults Invalid, obtive {other:?}"),
    }

    let totp_path = dir.path().join("gui_totp.png");
    let totp_uri =
        "otpauth://totp/Example%3Aalice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";
    save_qr_image(totp_uri, &totp_path, ImageFormat::Png);
    state.process_qr_path(&totp_path);
    match &state.result {
        GuiResult::QrResults {
            source_label,
            items,
        } => {
            assert_eq!(source_label, "gui_totp.png");
            assert_eq!(items.len(), 1);
            match &items[0] {
                QrClassification::OtpAuth(account) => {
                    assert_eq!(account.otp_type, OtpType::Totp);
                    assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
                }
                other => panic!("esperava OtpAuth, obtive {other:?}"),
            }
        }
        other => panic!("esperava QrResults OtpAuth, obtive {other:?}"),
    }
}

#[test]
fn gui_generated_qr_roundtrips_back_to_same_otpauth_account() {
    let uri = "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP&digits=6";
    let color_img = generate_qr_color_image(uri).expect("gerar QR ColorImage para conta");
    let [width, height] = color_img.size;

    let mut gray: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let px = color_img.pixels[y * width + x];
            gray.put_pixel(x as u32, y as u32, Luma([px.r()]));
        }
    }

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("roundtrip_qr.png");
    gray.save_with_format(&path, ImageFormat::Png).unwrap();

    let decoded = read_and_classify_qr_image(&path).expect("reler QR gerado pela GUI");
    assert_eq!(decoded.len(), 1);
    match &decoded[0] {
        QrClassification::OtpAuth(account) => {
            assert_eq!(account.otp_type, OtpType::Totp);
            assert_eq!(account.label, "Example:alice@google.com");
            assert_eq!(account.issuer.as_deref(), Some("Example"));
            assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
            assert_eq!(account.digits, Some(6));
        }
        other => panic!("esperava OtpAuth no round-trip, obtive {other:?}"),
    }

    // Verifica privacidade padrão do QR Code na GUI
    let mut state = GuiState::default();
    assert!(!state.reveal_secrets);
    assert!(state.revealed_qr_ids.is_empty());
    state.toggle_qr_visibility(10_000);
    assert!(state.revealed_qr_ids.contains(&10_000));
    state.toggle_qr_visibility(10_000);
    assert!(!state.revealed_qr_ids.contains(&10_000));
}

#[test]
fn gui_about_tab_and_metadata_match_otp_tools_requirements() {
    let mut state = GuiState::default();
    state.active_tab = GuiTab::About;
    assert_eq!(state.active_tab, GuiTab::About);
    assert_eq!(APP_DISPLAY_NAME, "OTP Tools");
    assert_eq!(APP_AUTHOR, "Glauco G. Cetara");
    assert_eq!(APP_EMAIL, "glaucogcetara@gmail.com");
    assert_eq!(APP_REPO_URL, "https://github.com/c3t4r4/otp-tools.git");
    assert_eq!(APP_LICENSE, "MIT License");
}

fn build_synthetic_camera_frame(content: &str) -> CameraFrame {
    let code = QrCode::new(content.as_bytes()).expect("gerar QR code simulado de câmera");
    let module_size = 8usize;
    let quiet_zone = 4usize;
    let side = (code.width() + 2 * quiet_zone) * module_size;
    let mut rgba = vec![255u8; side * side * 4];

    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                let px = (x + quiet_zone) * module_size;
                let py = (y + quiet_zone) * module_size;
                for dy in 0..module_size {
                    for dx in 0..module_size {
                        let idx = ((py + dy) * side + (px + dx)) * 4;
                        rgba[idx] = 0;
                        rgba[idx + 1] = 0;
                        rgba[idx + 2] = 0;
                        rgba[idx + 3] = 255;
                    }
                }
            }
        }
    }

    CameraFrame {
        width: side as u32,
        height: side as u32,
        rgba,
    }
}

#[test]
fn camera_frames_classify_totp_migration_and_invalid_and_feed_gui_state() {
    let totp_uri = "otpauth://totp/Phone%3Abob%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Phone";
    let totp_frame = build_synthetic_camera_frame(totp_uri);
    let totp_res =
        classify_qr_from_rgba_frame(totp_frame.width, totp_frame.height, &totp_frame.rgba)
            .expect("classificar TOTP de frame da câmera");
    assert!(matches!(
        &totp_res[0],
        QrClassification::OtpAuth(acc) if acc.otp_type == OtpType::Totp && acc.secret == "JBSWY3DPEHPK3PXP"
    ));

    let invalid_frame = build_synthetic_camera_frame("https://example.org/scan\x1b[31mANSI");
    let invalid_res = classify_qr_from_rgba_frame(
        invalid_frame.width,
        invalid_frame.height,
        &invalid_frame.rgba,
    )
    .expect("classificar QR inválido de frame");
    assert!(matches!(&invalid_res[0], QrClassification::Invalid { .. }));
    let rendered = render_qr_classification(&invalid_res[0], 1, 1);
    assert!(!rendered.contains("\x1b[31mANSI"));

    let blank = CameraFrame {
        width: 128,
        height: 128,
        rgba: vec![240u8; 128 * 128 * 4],
    };
    let mig_frame = build_synthetic_camera_frame(EXAMPLE_MIGRATION);
    let mut session = CameraSession::from_mock_frames(vec![blank, mig_frame]);
    let mut decoded_items = None;
    for _ in 0..100 {
        for update in session.poll() {
            if let CameraUpdate::Decoded { items, .. } = update {
                decoded_items = Some(items);
            }
        }
        if decoded_items.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    session.stop();

    let items = decoded_items.expect("sessão de câmera simulada deveria decodificar migration");
    let mut gui_state = GuiState::default();
    gui_state.process_camera_classifications(items);

    match &gui_state.result {
        GuiResult::QrResults {
            source_label,
            items,
        } => {
            assert_eq!(source_label, "Câmera (Ao vivo)");
            assert_eq!(items.len(), 1);
            assert!(matches!(
                &items[0],
                QrClassification::Migration { accounts, .. } if accounts.len() == 1
            ));
        }
        other => panic!("esperava QrResults da câmera, obtive {other:?}"),
    }

    gui_state.clear_all();
    assert_eq!(gui_state.result, GuiResult::Empty);
}

#[test]
fn cli_qr_camera_flags_and_timeout_validation() {
    let bin = env!("CARGO_BIN_EXE_otp-tools");

    let help = Command::new(bin)
        .args(["qr", "--help"])
        .output()
        .expect("executar qr --help");
    assert!(help.status.success());
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("--camera"));
    assert!(help_stdout.contains("--timeout"));

    let zero_timeout = Command::new(bin)
        .args(["qr", "--camera", "--timeout", "0"])
        .output()
        .expect("executar qr --camera --timeout 0");
    assert!(!zero_timeout.status.success());
    let stderr = String::from_utf8_lossy(&zero_timeout.stderr);
    assert!(stderr.contains("tempo limite") || stderr.contains("timeout"));

    let extreme_timeout = Command::new(bin)
        .args(["qr", "--camera", "--timeout", "9999999999"])
        .output()
        .expect("executar qr --camera --timeout extremo");
    assert!(!extreme_timeout.status.success());
    let extreme_stderr = String::from_utf8_lossy(&extreme_timeout.stderr);
    assert!(extreme_stderr.contains("3600"));

    let missing_arg = Command::new(bin)
        .args(["qr"])
        .output()
        .expect("executar qr sem arquivo nem câmera");
    assert!(!missing_arg.status.success());
}

#[test]
fn macos_app_icon_icns_contains_retina_and_small_finder_list_chunks() {
    let icns_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/AppIcon.icns");
    let data = fs::read(&icns_path).expect("ler assets/AppIcon.icns");
    assert!(data.len() > 8, "icns muito curto");
    assert_eq!(&data[0..4], b"icns", "magic header deve ser 'icns'");
    let declared_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    assert_eq!(
        declared_len,
        data.len(),
        "tamanho declarado no header icns deve bater"
    );

    let mut pos = 8usize;
    let mut chunk_tags: Vec<[u8; 4]> = Vec::new();
    while pos < data.len() {
        assert!(pos + 8 <= data.len(), "chunk header truncado");
        let tag: [u8; 4] = data[pos..pos + 4].try_into().expect("tag 4 bytes");
        let size = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        assert!(
            size >= 8 && pos + size <= data.len(),
            "tamanho inválido de chunk"
        );
        let payload = &data[pos + 8..pos + size];
        // Garante que chunks PNG (ic07..ic14) não possuem perfil iCCP inflado que corrompe miniaturas
        if payload.starts_with(b"\x89PNG\r\n\x1a\n") {
            let mut png_pos = 8usize;
            while png_pos + 12 <= payload.len() {
                let c_len = u32::from_be_bytes([
                    payload[png_pos],
                    payload[png_pos + 1],
                    payload[png_pos + 2],
                    payload[png_pos + 3],
                ]) as usize;
                let c_type = &payload[png_pos + 4..png_pos + 8];
                assert_ne!(
                    c_type,
                    b"iCCP",
                    "chunk {:?} não deve embutir perfil iCCP",
                    std::str::from_utf8(&tag)
                );
                png_pos = png_pos.saturating_add(12).saturating_add(c_len);
            }
        }
        chunk_tags.push(tag);
        pos += size;
    }

    for required in [
        *b"TOC ", *b"is32", *b"s8mk", *b"il32", *b"l8mk", *b"ic07", *b"ic08", *b"ic09", *b"ic10",
        *b"ic11", *b"ic12", *b"ic13", *b"ic14",
    ] {
        assert!(
            chunk_tags.contains(&required),
            "chunk obrigatório {:?} ausente em AppIcon.icns",
            std::str::from_utf8(&required)
        );
    }
    assert!(
        !chunk_tags.contains(b"icp4"),
        "icp4 legado corrompido não deve estar presente"
    );
    assert!(
        !chunk_tags.contains(b"icp5"),
        "icp5 legado corrompido não deve estar presente"
    );
}

#[cfg(unix)]
#[test]
fn release_script_auto_increments_blocks_downgrade_requires_confirm_and_pushes_tag() {
    use std::io::Write;
    use std::process::{Output, Stdio};

    fn run_git(dir: &Path, args: &[&str]) -> Output {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("executar comando git");
        assert!(
            out.status.success(),
            "git {:?} falhou em {:?}: {}",
            args,
            dir,
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn run_release(script: &Path, work_dir: &Path, args: &[&str], stdin_input: &str) -> Output {
        let mut cmd_args: Vec<&str> = Vec::with_capacity(args.len() + 3);
        cmd_args.extend_from_slice(args);
        cmd_args.push("--skip-build");
        cmd_args.push("--repo-root");
        cmd_args.push(work_dir.to_str().expect("caminho UTF-8"));

        let mut child = Command::new(script)
            .args(&cmd_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("executar ./release.sh");
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(stdin_input.as_bytes())
                .expect("escrever confirmação no stdin");
        }
        child
            .wait_with_output()
            .expect("aguardar saída de ./release.sh")
    }

    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("release.sh");
    assert!(script.is_file(), "release.sh deve existir na raiz");

    let temp = TempDir::new().expect("criar TempDir");
    let bare = temp.path().join("origin.git");
    let work = temp.path().join("work");
    fs::create_dir_all(&bare).expect("criar origin.git");
    fs::create_dir_all(&work).expect("criar work");

    run_git(&bare, &["init", "--bare"]);
    run_git(&work, &["init"]);
    run_git(&work, &["checkout", "-B", "main"]);
    run_git(&work, &["config", "user.email", "release-test@example.com"]);
    run_git(&work, &["config", "user.name", "Release Tester"]);
    run_git(
        &work,
        &[
            "remote",
            "add",
            "origin",
            bare.to_str().expect("bare path UTF-8"),
        ],
    );

    fs::write(
        work.join("Cargo.toml"),
        "[package]\nname = \"otp-tools\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1.0.229\"\n",
    )
    .expect("gravar Cargo.toml inicial");
    fs::write(
        work.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"otp-tools\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.229\"\n",
    )
    .expect("gravar Cargo.lock inicial");

    run_git(&work, &["add", "Cargo.toml", "Cargo.lock"]);
    run_git(&work, &["commit", "-m", "chore: initial commit"]);
    run_git(&work, &["push", "-u", "origin", "main"]);

    // 1. Cancelamento na confirmação não altera arquivos nem cria tag
    let cancelled = run_release(&script, &work, &["1.0.0"], "n\n");
    assert!(
        !cancelled.status.success(),
        "cancelamento deve retornar falha"
    );
    assert!(
        fs::read_to_string(work.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.1.0\""),
        "Cargo.toml deve permanecer em 0.1.0 após cancelamento"
    );
    assert!(
        String::from_utf8_lossy(&run_git(&bare, &["tag", "-l"]).stdout)
            .trim()
            .is_empty(),
        "nenhuma tag deve existir em origin.git após cancelamento"
    );

    // 2. Bloqueia versão menor do que a atual em Cargo.toml
    let lower_than_toml = run_release(&script, &work, &["0.0.9", "--yes"], "");
    assert!(!lower_than_toml.status.success());

    // 3. Sem informar versão, incrementa +1 no patch (0.1.0 -> 0.1.1) após confirmação
    let auto_first = run_release(&script, &work, &[], "s\n");
    assert!(
        auto_first.status.success(),
        "auto-incremento deveria passar: {}",
        String::from_utf8_lossy(&auto_first.stderr)
    );
    let toml_011 = fs::read_to_string(work.join("Cargo.toml")).unwrap();
    assert!(toml_011.contains("version = \"0.1.1\""));
    assert!(
        toml_011.contains("serde = \"1.0.229\""),
        "versão de dependência não pode ser alterada"
    );
    let lock_011 = fs::read_to_string(work.join("Cargo.lock")).unwrap();
    assert!(lock_011.contains("name = \"otp-tools\"\nversion = \"0.1.1\""));
    assert!(lock_011.contains("name = \"serde\"\nversion = \"1.0.229\""));

    let tags_after_first = String::from_utf8_lossy(&run_git(&bare, &["tag", "-l"]).stdout)
        .trim()
        .to_string();
    assert_eq!(tags_after_first, "v0.1.1");

    // 4. Bloqueia versão menor ou igual à já lançada (0.1.0, 0.1.1) e SemVer inválido
    assert!(!run_release(&script, &work, &["0.1.0", "--yes"], "")
        .status
        .success());
    assert!(!run_release(&script, &work, &["0.1.1", "--yes"], "")
        .status
        .success());
    assert!(!run_release(&script, &work, &["01.2.0", "--yes"], "")
        .status
        .success());

    // 5. Versão explícita v1.0.0 atualiza o app e envia tag v1.0.0 para origin
    let explicit_100 = run_release(&script, &work, &["v1.0.0"], "sim\n");
    assert!(
        explicit_100.status.success(),
        "release v1.0.0 falhou: {}",
        String::from_utf8_lossy(&explicit_100.stderr)
    );
    assert!(fs::read_to_string(work.join("Cargo.toml"))
        .unwrap()
        .contains("version = \"1.0.0\""));
    assert!(fs::read_to_string(work.join("Cargo.lock"))
        .unwrap()
        .contains("name = \"otp-tools\"\nversion = \"1.0.0\""));

    let bare_tags = String::from_utf8_lossy(&run_git(&bare, &["tag", "-l"]).stdout).to_string();
    assert!(bare_tags.lines().any(|line| line == "v1.0.0"));

    // 6. Auto-incremento após v1.0.0 gera v1.0.1
    let auto_101 = run_release(&script, &work, &["--yes"], "");
    assert!(
        auto_101.status.success(),
        "auto-incremento v1.0.1 falhou: {}",
        String::from_utf8_lossy(&auto_101.stderr)
    );
    assert!(fs::read_to_string(work.join("Cargo.toml"))
        .unwrap()
        .contains("version = \"1.0.1\""));
    let bare_tags_final =
        String::from_utf8_lossy(&run_git(&bare, &["tag", "-l"]).stdout).to_string();
    assert!(bare_tags_final.lines().any(|line| line == "v1.0.1"));

    // 7. Também bloqueia downgrade em relação a tag existente somente em origin (não no clone local)
    run_git(&bare, &["tag", "v2.0.0", "refs/heads/main"]);
    let blocked_by_origin = run_release(&script, &work, &["1.5.0", "--yes"], "");
    assert!(
        !blocked_by_origin.status.success(),
        "deveria bloquear 1.5.0 pois origin já possui v2.0.0"
    );

    // 8. Rejeita alteração não commitada no working tree ou index antes de tocar versão
    let orig_toml = fs::read_to_string(work.join("Cargo.toml")).unwrap();
    fs::write(work.join("Cargo.toml"), format!("{orig_toml}\n# dirty\n"))
        .expect("sujar Cargo.toml");
    let dirty_res = run_release(&script, &work, &["2.1.0", "--yes"], "");
    assert!(
        !dirty_res.status.success(),
        "deveria bloquear working tree com modificações não commitadas"
    );
    fs::write(work.join("Cargo.toml"), &orig_toml).expect("restaurar Cargo.toml");

    // 9. Rejeita detached HEAD
    run_git(&work, &["checkout", "--detach", "HEAD"]);
    let detached_res = run_release(&script, &work, &["2.1.0", "--yes"], "");
    assert!(
        !detached_res.status.success(),
        "deveria bloquear detached HEAD"
    );
    run_git(&work, &["checkout", "main"]);

    // 10. Falha fechada (fail-closed) quando o remoto 'origin' está inacessível
    let saved_origin =
        String::from_utf8_lossy(&run_git(&work, &["remote", "get-url", "origin"]).stdout)
            .trim()
            .to_string();
    let missing_origin = temp.path().join("missing_origin.git");
    run_git(
        &work,
        &[
            "remote",
            "set-url",
            "origin",
            missing_origin.to_str().expect("missing_origin UTF-8"),
        ],
    );
    let offline_res = run_release(&script, &work, &["2.1.0", "--yes"], "");
    assert!(
        !offline_res.status.success(),
        "deveria falhar em modo fail-closed quando origin estiver inacessível"
    );
    assert!(
        !fs::read_to_string(work.join("Cargo.toml"))
            .unwrap()
            .contains("2.1.0"),
        "não deveria alterar Cargo.toml quando origin estiver inacessível"
    );
    run_git(&work, &["remote", "set-url", "origin", &saved_origin]);
}
