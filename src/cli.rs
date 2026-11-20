use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::camera::scan_qr_from_camera_blocking;
use crate::error::AppError;
use crate::migration::decode_migration_uri;
use crate::model::{sanitize_single_line_terminal_text, sanitize_terminal_text, QrClassification};
use crate::otpauth::parse_otpauth_uri;
use crate::qrcode::read_and_classify_qr_image;
use crate::ui::{render_account, render_migration_accounts, render_qr_classification};

const MAX_CAMERA_TIMEOUT_SECS: u64 = 3600;

#[derive(Debug, Parser)]
#[command(
    name = "otp-tools",
    version,
    about = "OTP Tools — Decodifica links otpauth-migration://, analisa otpauth:// e lê QR Codes (PNG/JPG/JPEG ou câmera)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Lê um QR Code em PNG, JPG, JPEG ou pela câmera do PC e exibe os dados de otpauth:// ou otpauth-migration://
    Qr {
        /// Caminho do arquivo de imagem (PNG, JPG ou JPEG)
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,

        /// Caminho do arquivo de imagem (alternativa nomeada)
        #[arg(short = 'f', long = "file", conflicts_with = "file")]
        file_opt: Option<PathBuf>,

        /// Aciona a câmera/webcam do computador para ler o QR Code ao vivo
        #[arg(short = 'c', long = "camera", conflicts_with_all = ["file", "file_opt"])]
        camera: bool,

        /// Tempo máximo de espera (em segundos) ao ler pela câmera
        #[arg(long = "timeout", default_value_t = 30, requires = "camera")]
        timeout_secs: u64,

        /// Imprime apenas os links otpauth:// (modo texto simples)
        #[arg(long)]
        plain: bool,
    },

    /// Decodifica um link otpauth-migration://offline?data=... em contas otpauth://
    Decode {
        /// Link otpauth-migration://offline?data=...
        #[arg(value_name = "MIGRATION")]
        migration: Option<String>,

        /// Link otpauth-migration:// (alternativa nomeada compatível com o projeto Python)
        #[arg(short = 'm', long = "migration", conflicts_with = "migration")]
        migration_opt: Option<String>,

        /// Imprime apenas os links otpauth:// resultantes (um por linha)
        #[arg(long)]
        plain: bool,
    },

    /// Analisa um link otpauth://totp/... ou otpauth://hotp/... e exibe seus campos separados
    Parse {
        /// Link otpauth://totp/... ou otpauth://hotp/...
        uri: String,

        /// Imprime apenas o link otpauth://
        #[arg(long)]
        plain: bool,
    },
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Some(Commands::Qr {
            file,
            file_opt,
            camera,
            timeout_secs,
            plain,
        }) => {
            if camera {
                handle_camera_qr(timeout_secs, plain, true)
            } else {
                let path = file.or(file_opt).ok_or_else(|| {
                    AppError::ImageReadError(
                        "Informe o caminho da imagem ou use --camera: otp-tools qr <ARQUIVO> | --camera"
                            .to_string(),
                    )
                })?;
                handle_qr(&path, plain)
            }
        }
        Some(Commands::Decode {
            migration,
            migration_opt,
            plain,
        }) => {
            let uri = migration
                .or(migration_opt)
                .ok_or(AppError::InvalidMigrationUri)?;
            handle_decode(&uri, plain)
        }
        Some(Commands::Parse { uri, plain }) => handle_parse(&uri, plain),
        None => run_interactive(),
    }
}

fn handle_qr(path: &Path, plain: bool) -> Result<(), AppError> {
    let classifications = read_and_classify_qr_image(path)?;
    print_classifications(&classifications, plain);
    Ok(())
}

fn handle_camera_qr(timeout_secs: u64, plain: bool, announce: bool) -> Result<(), AppError> {
    if timeout_secs == 0 || timeout_secs > MAX_CAMERA_TIMEOUT_SECS {
        return Err(AppError::CameraError(format!(
            "O tempo limite (--timeout) deve estar entre 1 e {MAX_CAMERA_TIMEOUT_SECS} segundos."
        )));
    }
    if announce && !plain {
        eprintln!(
            "Câmera ativada. Aponte o QR Code do celular para a webcam (limite: {}s)...",
            timeout_secs
        );
    }
    let classifications = scan_qr_from_camera_blocking(Duration::from_secs(timeout_secs))?;
    print_classifications(&classifications, plain);
    Ok(())
}

fn print_classifications(classifications: &[QrClassification], plain: bool) {
    let total = classifications.len();

    for (idx, item) in classifications.iter().enumerate() {
        if plain {
            print_classification_plain(item);
        } else {
            print!("{}", render_qr_classification(item, idx + 1, total));
        }
    }
}

fn handle_decode(uri: &str, plain: bool) -> Result<(), AppError> {
    let accounts = decode_migration_uri(uri)?;
    if plain {
        for account in &accounts {
            println!("{}", sanitize_single_line_terminal_text(&account.uri));
        }
    } else {
        print!("{}", render_migration_accounts(&accounts));
    }
    Ok(())
}

fn handle_parse(uri: &str, plain: bool) -> Result<(), AppError> {
    let account = parse_otpauth_uri(uri)?;
    if plain {
        println!("{}", sanitize_single_line_terminal_text(&account.uri));
    } else {
        print!("{}", render_account(&account, None));
    }
    Ok(())
}

fn print_classification_plain(item: &QrClassification) {
    match item {
        QrClassification::OtpAuth(account) => {
            println!("{}", sanitize_single_line_terminal_text(&account.uri));
        }
        QrClassification::Migration { accounts, .. } => {
            for account in accounts {
                println!("{}", sanitize_single_line_terminal_text(&account.uri));
            }
        }
        QrClassification::Invalid {
            raw_content,
            reason,
        } => {
            eprintln!("QR Code inválido: {}", sanitize_terminal_text(reason));
            println!("{}", sanitize_single_line_terminal_text(raw_content));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractiveAction {
    ReadQr,
    ScanCameraQr,
    DecodeMigration,
    ParseOtpAuth,
    Exit,
}

fn run_interactive() -> Result<(), AppError> {
    cliclack::intro("OTP Tools — Leitor de QR Code e Decoder OTP")?;

    loop {
        let action = cliclack::select("O que você deseja fazer?")
            .item(
                InteractiveAction::ReadQr,
                "Ler QR Code de imagem (PNG, JPG, JPEG)",
                "Detecta otpauth://totp/, otpauth://hotp/, otpauth-migration:// ou exibe conteúdo inválido",
            )
            .item(
                InteractiveAction::ScanCameraQr,
                "Ler QR Code pela câmera / webcam",
                "Ativa a câmera do PC para escanear o QR Code exibido no celular",
            )
            .item(
                InteractiveAction::DecodeMigration,
                "Decodificar link otpauth-migration://",
                "Extrai todas as contas do payload Protobuf do Google Authenticator",
            )
            .item(
                InteractiveAction::ParseOtpAuth,
                "Analisar link otpauth:// (TOTP / HOTP)",
                "Separa e exibe todos os campos do link",
            )
            .item(InteractiveAction::Exit, "Sair", "Encerra o programa")
            .interact()?;

        match action {
            InteractiveAction::ReadQr => {
                let raw_path: String = cliclack::input("Caminho da imagem (PNG, JPG ou JPEG):")
                    .placeholder("./qrcode.png")
                    .validate(|input: &String| {
                        if input.trim().is_empty() {
                            Err("Informe o caminho de um arquivo de imagem.")
                        } else {
                            Ok(())
                        }
                    })
                    .interact()?;

                let path = PathBuf::from(raw_path.trim().trim_matches(|c| c == '"' || c == '\''));
                match handle_qr(&path, false) {
                    Ok(()) => {}
                    Err(err) => {
                        cliclack::log::error(sanitize_terminal_text(&err.to_string()))?;
                    }
                }
            }
            InteractiveAction::ScanCameraQr => {
                cliclack::log::info(
                    "Câmera ativada. Aponte o QR Code do celular para a webcam (limite: 30s)...",
                )?;
                match handle_camera_qr(30, false, false) {
                    Ok(()) => {}
                    Err(err) => {
                        cliclack::log::error(sanitize_terminal_text(&err.to_string()))?;
                    }
                }
            }
            InteractiveAction::DecodeMigration => {
                let uri: String =
                    cliclack::input("Cole o link otpauth-migration://offline?data=...:")
                        .validate(|input: &String| {
                            if input.trim().is_empty() {
                                Err("Informe um link otpauth-migration://.")
                            } else {
                                Ok(())
                            }
                        })
                        .interact()?;

                match handle_decode(&uri, false) {
                    Ok(()) => {}
                    Err(err) => {
                        cliclack::log::error(sanitize_terminal_text(&err.to_string()))?;
                    }
                }
            }
            InteractiveAction::ParseOtpAuth => {
                let uri: String =
                    cliclack::input("Cole o link otpauth://totp/... ou otpauth://hotp/...:")
                        .validate(|input: &String| {
                            if input.trim().is_empty() {
                                Err("Informe um link otpauth://.")
                            } else {
                                Ok(())
                            }
                        })
                        .interact()?;

                match handle_parse(&uri, false) {
                    Ok(()) => {}
                    Err(err) => {
                        cliclack::log::error(sanitize_terminal_text(&err.to_string()))?;
                    }
                }
            }
            InteractiveAction::Exit => {
                cliclack::outro("Até logo!")?;
                break;
            }
        }
    }

    Ok(())
}
