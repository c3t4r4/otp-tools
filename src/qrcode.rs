use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use image::{DynamicImage, GrayImage, ImageFormat, ImageReader, Limits, Luma};

use crate::error::AppError;
use crate::migration::decode_migration_uri;
use crate::model::QrClassification;
use crate::otpauth::parse_otpauth_uri;

pub const MAX_IMAGE_FILE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
pub const MAX_IMAGE_DIMENSION: u32 = 10_000;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024; // 256 MB
const ESTIMATED_BYTES_PER_PIXEL: u64 = 6;

/// Classifica o texto extraído de um QR Code entre `otpauth://` (TOTP/HOTP),
/// `otpauth-migration://` ou inválido (preservando o conteúdo bruto).
pub fn classify_qr_payload(raw_content: &str) -> QrClassification {
    let trimmed = raw_content.trim();
    let lower = trimmed.to_ascii_lowercase();

    if lower.starts_with("otpauth-migration://") {
        return match decode_migration_uri(trimmed) {
            Ok(accounts) => QrClassification::Migration {
                raw_uri: trimmed.to_string(),
                accounts,
            },
            Err(err) => QrClassification::Invalid {
                raw_content: raw_content.to_string(),
                reason: format!("Link otpauth-migration inválido: {err}"),
            },
        };
    }

    if lower.starts_with("otpauth://totp/") || lower.starts_with("otpauth://hotp/") {
        return match parse_otpauth_uri(trimmed) {
            Ok(account) => QrClassification::OtpAuth(account),
            Err(err) => QrClassification::Invalid {
                raw_content: raw_content.to_string(),
                reason: format!("Link otpauth inválido: {err}"),
            },
        };
    }

    QrClassification::Invalid {
        raw_content: raw_content.to_string(),
        reason:
            "Conteúdo não corresponde a otpauth://totp/, otpauth://hotp/ nem otpauth-migration://"
                .to_string(),
    }
}

/// Lê uma imagem PNG, JPG ou JPEG, extrai todos os QR Codes presentes e classifica cada um.
pub fn read_and_classify_qr_image(path: &Path) -> Result<Vec<QrClassification>, AppError> {
    let payloads = extract_qr_payloads_from_image(path)?;
    Ok(payloads
        .iter()
        .map(|payload| classify_qr_payload(payload))
        .collect())
}

/// Extrai os payloads brutos de todos os QR Codes contidos em uma imagem PNG, JPG ou JPEG.
pub fn extract_qr_payloads_from_image(path: &Path) -> Result<Vec<String>, AppError> {
    let bytes = read_bounded_image_bytes(path)?;

    let reader = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|err| AppError::ImageReadError(err.to_string()))?;

    let format = reader.format().ok_or_else(|| {
        AppError::UnsupportedImageFormat("não foi possível identificar o formato".to_string())
    })?;

    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(AppError::UnsupportedImageFormat(format!("{format:?}")));
    }

    let mut dim_reader = reader;
    dim_reader.limits(image_decode_limits());

    let (width, height) = dim_reader
        .into_dimensions()
        .map_err(|err| AppError::ImageReadError(err.to_string()))?;

    validate_image_dimensions(width, height)?;

    let mut decode_reader = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|err| AppError::ImageReadError(err.to_string()))?;
    decode_reader.limits(image_decode_limits());

    let dynamic_img = decode_reader
        .decode()
        .map_err(|err| AppError::ImageReadError(err.to_string()))?;

    let gray_img = to_luma8_with_white_bg(&dynamic_img);
    extract_qr_payloads_from_gray_image(gray_img).map_err(|err| match err {
        AppError::NoQrCodeFoundInFrame => AppError::NoQrCodeFound(path.to_path_buf()),
        other => other,
    })
}

/// Extrai os payloads brutos de todos os QR Codes presentes em uma imagem em escala de cinza.
pub fn extract_qr_payloads_from_gray_image(gray_img: GrayImage) -> Result<Vec<String>, AppError> {
    let (width, height) = gray_img.dimensions();
    validate_image_dimensions(width, height)?;

    let mut prepared = rqrr::PreparedImage::prepare(gray_img);
    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err(AppError::NoQrCodeFoundInFrame);
    }

    let mut results = Vec::with_capacity(grids.len());
    let mut last_error: Option<String> = None;

    for grid in grids {
        match grid.decode() {
            Ok((_, content)) => results.push(content),
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    if results.is_empty() {
        return Err(AppError::QrDecodeError(last_error.unwrap_or_else(|| {
            "não foi possível decodificar o QR Code".to_string()
        })));
    }

    Ok(results)
}

/// Extrai e classifica todos os QR Codes presentes em uma imagem em escala de cinza.
pub fn classify_qr_from_gray_image(gray_img: GrayImage) -> Result<Vec<QrClassification>, AppError> {
    let payloads = extract_qr_payloads_from_gray_image(gray_img)?;
    Ok(payloads
        .iter()
        .map(|payload| classify_qr_payload(payload))
        .collect())
}

/// Extrai e classifica todos os QR Codes presentes em um frame RGBA8 (`width * height * 4`).
pub fn classify_qr_from_rgba_frame(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<Vec<QrClassification>, AppError> {
    validate_image_dimensions(width, height)?;

    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;

    if rgba.len() != expected_len {
        return Err(AppError::ImageReadError(format!(
            "tamanho do buffer RGBA ({}) não corresponde a {}x{}x4",
            rgba.len(),
            width,
            height
        )));
    }

    let mut gray_bytes = Vec::with_capacity((width as usize) * (height as usize));
    for &[r, g, b, a] in rgba.as_chunks::<4>().0 {
        let luma = if a == 255 {
            ((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000) as u8
        } else {
            let alpha = u32::from(a);
            let inv_alpha = 255 - alpha;
            let r_comp = (u32::from(r) * alpha + 255 * inv_alpha) / 255;
            let g_comp = (u32::from(g) * alpha + 255 * inv_alpha) / 255;
            let b_comp = (u32::from(b) * alpha + 255 * inv_alpha) / 255;
            ((299 * r_comp + 587 * g_comp + 114 * b_comp) / 1000) as u8
        };
        gray_bytes.push(luma);
    }

    let gray_img = GrayImage::from_raw(width, height, gray_bytes)
        .ok_or_else(|| AppError::ImageReadError("falha ao montar imagem em cinza".to_string()))?;

    classify_qr_from_gray_image(gray_img)
}

fn image_decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    limits
}

fn validate_image_dimensions(width: u32, height: u32) -> Result<(), AppError> {
    if width == 0 || height == 0 {
        return Err(AppError::ImageReadError(
            "dimensões de imagem inválidas (0px)".to_string(),
        ));
    }

    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        });
    }

    let estimated_bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(ESTIMATED_BYTES_PER_PIXEL);
    if estimated_bytes > MAX_DECODE_ALLOC_BYTES {
        return Err(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        });
    }

    Ok(())
}

/// Valida extensão e abre o arquivo uma única vez, lendo no máximo `MAX_IMAGE_FILE_BYTES`
/// para eliminar janela TOCTOU entre validação de tamanho e decodificação.
fn read_bounded_image_bytes(path: &Path) -> Result<Vec<u8>, AppError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::FileNotFound(path.to_path_buf()));
        }
        Err(err) => return Err(AppError::Io(err)),
    };

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::NotAFile(path.to_path_buf()));
    }

    if metadata.len() > MAX_IMAGE_FILE_BYTES {
        return Err(AppError::FileTooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_IMAGE_FILE_BYTES,
        });
    }

    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return Err(AppError::UnsupportedImageFormat(if ext.is_empty() {
            "sem extensão".to_string()
        } else {
            format!(".{ext}")
        }));
    }

    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_IMAGE_FILE_BYTES) as usize);
    file.take(MAX_IMAGE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;

    if bytes.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(AppError::FileTooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_IMAGE_FILE_BYTES,
        });
    }

    Ok(bytes)
}

/// Converte uma imagem para escala de cinza sem alocação RGBA desnecessária quando não há alfa,
/// compondo pixels transparentes sobre fundo branco apenas quando há canal alfa.
fn to_luma8_with_white_bg(img: &DynamicImage) -> GrayImage {
    match img {
        DynamicImage::ImageLuma8(gray) => gray.clone(),
        DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgb32F(_) => img.to_luma8(),
        _ => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let mut gray = GrayImage::new(width, height);

            for (x, y, pixel) in rgba.enumerate_pixels() {
                let [r, g, b, a] = pixel.0;
                let alpha = u32::from(a);
                let inv_alpha = 255 - alpha;

                let r_comp = (u32::from(r) * alpha + 255 * inv_alpha) / 255;
                let g_comp = (u32::from(g) * alpha + 255 * inv_alpha) / 255;
                let b_comp = (u32::from(b) * alpha + 255 * inv_alpha) / 255;

                // Luminância Rec. 601
                let luma = ((299 * r_comp + 587 * g_comp + 114 * b_comp) / 1000) as u8;
                gray.put_pixel(x, y, Luma([luma]));
            }

            gray
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXAMPLE_MIGRATION;
    use crate::model::OtpType;

    #[test]
    fn classify_totp_payload() {
        let uri = "otpauth://totp/Example:alice@google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";
        match classify_qr_payload(uri) {
            QrClassification::OtpAuth(account) => {
                assert_eq!(account.otp_type, OtpType::Totp);
                assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
            }
            other => panic!("esperava OtpAuth, obtive {other:?}"),
        }
    }

    #[test]
    fn classify_hotp_payload() {
        let uri = "otpauth://hotp/Example:bob@google.com?secret=JBSWY3DPEHPK3PXP&counter=5";
        match classify_qr_payload(uri) {
            QrClassification::OtpAuth(account) => {
                assert_eq!(account.otp_type, OtpType::Hotp);
                assert_eq!(account.counter, Some(5));
            }
            other => panic!("esperava OtpAuth HOTP, obtive {other:?}"),
        }
    }

    #[test]
    fn classify_migration_payload() {
        match classify_qr_payload(EXAMPLE_MIGRATION) {
            QrClassification::Migration { accounts, .. } => {
                assert_eq!(accounts.len(), 1);
                assert_eq!(accounts[0].secret, "JBSWY3DPEHPK3PXP");
            }
            other => panic!("esperava Migration, obtive {other:?}"),
        }
    }

    #[test]
    fn classify_invalid_payload_preserves_raw_content() {
        let raw = "https://example.com/hello?x=1";
        match classify_qr_payload(raw) {
            QrClassification::Invalid { raw_content, .. } => {
                assert_eq!(raw_content, raw);
            }
            other => panic!("esperava Invalid, obtive {other:?}"),
        }
    }

    #[test]
    fn validate_dimensions_rejects_zero_and_excessive_memory_budget() {
        assert!(validate_image_dimensions(0, 100).is_err());
        assert!(validate_image_dimensions(100, 0).is_err());
        assert!(validate_image_dimensions(10_000, 10_000).is_err());
        assert!(validate_image_dimensions(2_000, 2_000).is_ok());
    }

    fn build_rgba_qr_frame(content: &str) -> (u32, u32, Vec<u8>) {
        let code = ::qrcode::QrCode::new(content.as_bytes()).expect("gerar QR de teste");
        let module_size = 6usize;
        let quiet = 4usize;
        let side = (code.width() + 2 * quiet) * module_size;
        let mut rgba = vec![255u8; side * side * 4];
        for y in 0..code.width() {
            for x in 0..code.width() {
                if code[(x, y)] == ::qrcode::Color::Dark {
                    let px = (x + quiet) * module_size;
                    let py = (y + quiet) * module_size;
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
        (side as u32, side as u32, rgba)
    }

    #[test]
    fn classify_qr_from_rgba_frame_handles_totp_migration_blank_and_invalid_len() {
        let totp_uri =
            "otpauth://totp/Example%3Aalice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";
        let (w, h, rgba) = build_rgba_qr_frame(totp_uri);
        let decoded = classify_qr_from_rgba_frame(w, h, &rgba).expect("decodificar TOTP RGBA");
        assert_eq!(decoded.len(), 1);
        assert!(
            matches!(&decoded[0], QrClassification::OtpAuth(acc) if acc.secret == "JBSWY3DPEHPK3PXP")
        );

        let (mw, mh, mrgba) = build_rgba_qr_frame(EXAMPLE_MIGRATION);
        let decoded_mig =
            classify_qr_from_rgba_frame(mw, mh, &mrgba).expect("decodificar Migration RGBA");
        assert_eq!(decoded_mig.len(), 1);
        assert!(matches!(
            &decoded_mig[0],
            QrClassification::Migration { accounts, .. } if accounts.len() == 1
        ));

        let blank = vec![255u8; 64 * 64 * 4];
        let err = classify_qr_from_rgba_frame(64, 64, &blank).unwrap_err();
        assert!(matches!(err, AppError::NoQrCodeFoundInFrame));

        let bad_len = vec![255u8; 10];
        let err_len = classify_qr_from_rgba_frame(64, 64, &bad_len).unwrap_err();
        assert!(matches!(err_len, AppError::ImageReadError(_)));
    }
}
