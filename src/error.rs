use std::path::PathBuf;
use thiserror::Error;

pub const EXAMPLE_MIGRATION: &str =
    "otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC";

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Link de migração inválido. Esperado formato semelhante a \"{EXAMPLE_MIGRATION}\"")]
    InvalidMigrationUri,

    #[error("Link otpauth inválido: {0}")]
    InvalidOtpAuthUri(String),

    #[error("Falha ao decodificar Base64 do payload de migração: {0}")]
    InvalidBase64(String),

    #[error("Falha ao decodificar Protobuf do payload de migração: {0}")]
    InvalidProtobuf(#[from] prost::DecodeError),

    #[error("Arquivo não encontrado: {0}")]
    FileNotFound(PathBuf),

    #[error("O caminho informado não é um arquivo regular: {0}")]
    NotAFile(PathBuf),

    #[error("Arquivo excede o limite máximo permitido ({max_bytes} bytes): {path}")]
    FileTooLarge { path: PathBuf, max_bytes: u64 },

    #[error("Formato de imagem não suportado (aceitos: PNG, JPG, JPEG): {0}")]
    UnsupportedImageFormat(String),

    #[error("Dimensões de imagem excedem o limite máximo seguro ({max_dimension}px)")]
    ImageTooLarge { max_dimension: u32 },

    #[error("Falha ao ler arquivo de imagem: {0}")]
    ImageReadError(String),

    #[error("Nenhum QR Code foi encontrado na imagem: {0}")]
    NoQrCodeFound(PathBuf),

    #[error("Nenhum QR Code foi encontrado no quadro capturado")]
    NoQrCodeFoundInFrame,

    #[error("Falha ao decodificar conteúdo do QR Code: {0}")]
    QrDecodeError(String),

    #[error("Erro na câmera: {0}")]
    CameraError(String),

    #[error("Erro de entrada/saída: {0}")]
    Io(#[from] std::io::Error),
}
