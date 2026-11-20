use percent_encoding::percent_decode_str;
use url::Url;

use crate::error::AppError;
use crate::model::{sanitize_terminal_text, Algorithm, OtpAccount, OtpType};

pub const OTPAUTH_SCHEME: &str = "otpauth";

/// Analisa uma URI `otpauth://totp/...` ou `otpauth://hotp/...` em seus campos estruturados.
pub fn parse_otpauth_uri(raw: &str) -> Result<OtpAccount, AppError> {
    let trimmed = raw.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(AppError::InvalidOtpAuthUri(
            "URI contém caracteres de controle não permitidos".to_string(),
        ));
    }

    let parsed = Url::parse(trimmed)
        .map_err(|_| AppError::InvalidOtpAuthUri("URI malformada".to_string()))?;

    if parsed.scheme() != OTPAUTH_SCHEME {
        return Err(AppError::InvalidOtpAuthUri(format!(
            "esquema esperado '{OTPAUTH_SCHEME}', encontrado '{}'",
            sanitize_terminal_text(parsed.scheme())
        )));
    }

    let otp_type = match parsed.host_str().map(str::to_ascii_lowercase).as_deref() {
        Some("totp") => OtpType::Totp,
        Some("hotp") => OtpType::Hotp,
        other => {
            return Err(AppError::InvalidOtpAuthUri(format!(
                "tipo OTP não suportado: '{}'",
                sanitize_terminal_text(other.unwrap_or(""))
            )));
        }
    };

    let raw_label = parsed.path().trim_start_matches('/');
    let label = percent_decode_str(raw_label)
        .decode_utf8()
        .map_err(|_| AppError::InvalidOtpAuthUri("label com UTF-8 inválido".to_string()))?
        .into_owned();

    let mut secret: Option<String> = None;
    let mut issuer: Option<String> = None;
    let mut algorithm: Option<Algorithm> = None;
    let mut digits: Option<u32> = None;
    let mut period: Option<u32> = None;
    let mut counter: Option<i64> = None;

    for (key, value) in parsed.query_pairs() {
        let key_lower = key.to_ascii_lowercase();
        let val = value.trim();
        if val.is_empty() {
            continue;
        }
        match key_lower.as_str() {
            "secret" => secret = Some(val.to_string()),
            "issuer" => issuer = Some(val.to_string()),
            "algorithm" => {
                algorithm = Some(Algorithm::parse_str(val).ok_or_else(|| {
                    AppError::InvalidOtpAuthUri(format!(
                        "algoritmo desconhecido: '{}'",
                        sanitize_terminal_text(val)
                    ))
                })?);
            }
            "digits" => {
                let parsed_digits: u32 = val.parse().map_err(|_| {
                    AppError::InvalidOtpAuthUri(format!(
                        "digits inválido: '{}'",
                        sanitize_terminal_text(val)
                    ))
                })?;
                digits = Some(parsed_digits);
            }
            "period" => {
                let parsed_period: u32 = val.parse().map_err(|_| {
                    AppError::InvalidOtpAuthUri(format!(
                        "period inválido: '{}'",
                        sanitize_terminal_text(val)
                    ))
                })?;
                period = Some(parsed_period);
            }
            "counter" => {
                let parsed_counter: i64 = val.parse().map_err(|_| {
                    AppError::InvalidOtpAuthUri(format!(
                        "counter inválido: '{}'",
                        sanitize_terminal_text(val)
                    ))
                })?;
                counter = Some(parsed_counter);
            }
            _ => {}
        }
    }

    let secret = secret.ok_or_else(|| {
        AppError::InvalidOtpAuthUri("parâmetro obrigatório 'secret' ausente".to_string())
    })?;

    // Se issuer não veio na query, tenta inferir do prefixo "Issuer:account" do label.
    if issuer.is_none() {
        if let Some((prefix, _)) = label.split_once(':') {
            let candidate = prefix.trim();
            if !candidate.is_empty() {
                issuer = Some(candidate.to_string());
            }
        }
    }

    Ok(OtpAccount {
        otp_type,
        label,
        issuer,
        secret,
        algorithm,
        digits,
        period,
        counter,
        uri: parsed.as_str().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_totp_uri() {
        let uri = "otpauth://totp/Example%3Aalice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example&algorithm=SHA1&digits=6&period=30";
        let account = parse_otpauth_uri(uri).unwrap();
        assert_eq!(account.otp_type, OtpType::Totp);
        assert_eq!(account.label, "Example:alice@google.com");
        assert_eq!(account.issuer.as_deref(), Some("Example"));
        assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
        assert_eq!(account.algorithm, Some(Algorithm::Sha1));
        assert_eq!(account.digits, Some(6));
        assert_eq!(account.period, Some(30));
        assert_eq!(account.counter, None);
        assert_eq!(account.uri, uri);
    }

    #[test]
    fn parse_valid_hotp_uri_with_inferred_issuer() {
        let uri = "otpauth://hotp/GitHub:user%40example.com?secret=JBSWY3DPEHPK3PXP&counter=7";
        let account = parse_otpauth_uri(uri).unwrap();
        assert_eq!(account.otp_type, OtpType::Hotp);
        assert_eq!(account.label, "GitHub:user@example.com");
        assert_eq!(account.issuer.as_deref(), Some("GitHub"));
        assert_eq!(account.counter, Some(7));
    }

    #[test]
    fn reject_missing_secret() {
        let uri = "otpauth://totp/Example:alice?issuer=Example";
        assert!(parse_otpauth_uri(uri).is_err());
    }

    #[test]
    fn reject_unknown_otp_type() {
        let uri = "otpauth://steam/Example:alice?secret=JBSWY3DPEHPK3PXP";
        assert!(parse_otpauth_uri(uri).is_err());
    }

    #[test]
    fn reject_control_characters_in_uri() {
        let uri = "otpauth://totp/Acme?secret=JBSWY3DPEHPK3PXP&note=\x1b[31mINJECT";
        assert!(parse_otpauth_uri(uri).is_err());
    }

    #[test]
    fn error_message_sanitizes_percent_encoded_control_chars() {
        let uri = "otpauth://totp/Test?secret=JBSWY3DPEHPK3PXP&digits=%1B[31mFAKE";
        let err = parse_otpauth_uri(uri).unwrap_err().to_string();
        assert!(!err.contains('\x1b'));
        assert!(err.contains("\\u{001B}"));
    }
}
