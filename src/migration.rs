use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use data_encoding::BASE32_NOPAD;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use prost::Message;
use url::{form_urlencoded, Url};

use crate::error::AppError;
use crate::model::{Algorithm, OtpAccount, OtpType};
use crate::proto::{OtpParameters, Payload, ProtoAlgorithm, ProtoDigitCount, ProtoOtpType};

pub const MIGRATION_SCHEME: &str = "otpauth-migration";
pub const MIGRATION_HOST: &str = "offline";
pub const PAYLOAD_PARAM: &str = "data";

/// Mesmo conjunto seguro do `urllib.parse.quote` do Python (preserva `/`, `-`, `_`, `.`, `~`).
const PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Valida um link `otpauth-migration://offline?data=...` e retorna todos os valores `data`.
pub fn validate_migration_uri(migration: &str) -> Result<Vec<String>, AppError> {
    let trimmed = migration.trim();
    if trimmed.chars().any(char::is_control) {
        return Err(AppError::InvalidMigrationUri);
    }

    let parsed = Url::parse(trimmed).map_err(|_| AppError::InvalidMigrationUri)?;

    if parsed.scheme() != MIGRATION_SCHEME || parsed.host_str() != Some(MIGRATION_HOST) {
        return Err(AppError::InvalidMigrationUri);
    }

    let data_values: Vec<String> = parsed
        .query_pairs()
        .filter(|(k, v)| k == PAYLOAD_PARAM && !v.trim().is_empty())
        .map(|(_, v)| v.into_owned())
        .collect();

    if data_values.is_empty() {
        return Err(AppError::InvalidMigrationUri);
    }

    Ok(data_values)
}

/// Decodifica cada payload Base64 extraído do parâmetro `data`.
pub fn decode_base64_payloads(data_items: &[String]) -> Result<Vec<Vec<u8>>, AppError> {
    data_items
        .iter()
        .map(|item| decode_single_base64(item))
        .collect()
}

fn decode_single_base64(raw: &str) -> Result<Vec<u8>, AppError> {
    // Em query strings form-urlencoded, '+' não escapado vira espaço ' '.
    let normalized = raw.trim().replace(' ', "+");
    STANDARD
        .decode(&normalized)
        .or_else(|_| STANDARD_NO_PAD.decode(&normalized))
        .or_else(|_| URL_SAFE.decode(&normalized))
        .or_else(|_| URL_SAFE_NO_PAD.decode(&normalized))
        .map_err(|err| AppError::InvalidBase64(err.to_string()))
}

/// Converte os bytes da chave secreta em Base32 sem padding (`=`),
/// mantendo compatibilidade exata com `decode_secret` do projeto Python.
pub fn encode_secret(secret: &[u8]) -> String {
    BASE32_NOPAD.encode(secret)
}

/// Monta a query string dos parâmetros OTP seguindo a ordem do projeto de referência:
/// `algorithm`, `digits`, `issuer`, `secret` (e `counter` quando HOTP).
pub fn build_url_params(otp: &OtpParameters) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());

    if let Some(alg) = map_algorithm(otp.algorithm) {
        serializer.append_pair("algorithm", alg.as_str());
    }
    if let Some(digits) = map_digits(otp.digits) {
        serializer.append_pair("digits", &digits.to_string());
    }
    if !otp.issuer.is_empty() {
        serializer.append_pair("issuer", &otp.issuer);
    }
    if !otp.secret.is_empty() {
        let encoded_secret = encode_secret(&otp.secret);
        serializer.append_pair("secret", &encoded_secret);
    }
    if map_otp_type(otp.r#type) == Some(OtpType::Hotp) {
        serializer.append_pair("counter", &otp.counter.to_string());
    }

    serializer.finish()
}

/// Constrói a URI `otpauth://...` a partir dos parâmetros Protobuf.
pub fn build_otpauth_url(otp: &OtpParameters) -> String {
    let otp_type = map_otp_type(otp.r#type).unwrap_or(OtpType::Totp).as_str();
    let encoded_name = utf8_percent_encode(&otp.name, PATH_ENCODE_SET).to_string();
    let params = build_url_params(otp);

    format!("otpauth://{otp_type}/{encoded_name}?{params}")
}

/// Converte um `OtpParameters` do Protobuf em um `OtpAccount` completo.
pub fn otp_parameters_to_account(otp: &OtpParameters) -> Result<OtpAccount, AppError> {
    let otp_type = map_otp_type(otp.r#type).ok_or(AppError::InvalidMigrationUri)?;
    if otp.secret.is_empty() {
        return Err(AppError::InvalidMigrationUri);
    }

    let algorithm = map_algorithm(otp.algorithm);
    let digits = map_digits(otp.digits);
    let issuer = if otp.issuer.is_empty() {
        None
    } else {
        Some(otp.issuer.clone())
    };
    let secret = encode_secret(&otp.secret);
    let counter = if otp_type == OtpType::Hotp {
        Some(otp.counter)
    } else {
        None
    };
    let uri = build_otpauth_url(otp);

    Ok(OtpAccount {
        otp_type,
        label: otp.name.clone(),
        issuer,
        secret,
        algorithm,
        digits,
        period: None,
        counter,
        uri,
    })
}

/// Decodifica um link completo `otpauth-migration://offline?data=...` em todas as contas OTP contidas nele.
pub fn decode_migration_uri(migration: &str) -> Result<Vec<OtpAccount>, AppError> {
    let data_items = validate_migration_uri(migration)?;
    let raw_payloads = decode_base64_payloads(&data_items)?;

    let mut accounts = Vec::new();
    for raw_payload in raw_payloads {
        let payload = Payload::decode(raw_payload.as_slice())?;
        for otp in &payload.otp_parameters {
            accounts.push(otp_parameters_to_account(otp)?);
        }
    }

    if accounts.is_empty() {
        return Err(AppError::InvalidMigrationUri);
    }

    Ok(accounts)
}

fn map_algorithm(raw: i32) -> Option<Algorithm> {
    match ProtoAlgorithm::try_from(raw).ok()? {
        ProtoAlgorithm::Sha1 => Some(Algorithm::Sha1),
        ProtoAlgorithm::Sha256 => Some(Algorithm::Sha256),
        ProtoAlgorithm::Sha512 => Some(Algorithm::Sha512),
        ProtoAlgorithm::Md5 => Some(Algorithm::Md5),
        ProtoAlgorithm::Unspecified => None,
    }
}

fn map_digits(raw: i32) -> Option<u32> {
    match ProtoDigitCount::try_from(raw).ok()? {
        ProtoDigitCount::Six => Some(6),
        ProtoDigitCount::Eight => Some(8),
        ProtoDigitCount::Unspecified => None,
    }
}

fn map_otp_type(raw: i32) -> Option<OtpType> {
    match ProtoOtpType::try_from(raw).ok()? {
        ProtoOtpType::Hotp => Some(OtpType::Hotp),
        ProtoOtpType::Totp => Some(OtpType::Totp),
        ProtoOtpType::Unspecified => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXAMPLE_MIGRATION;

    #[test]
    fn test_encode_secret_matches_python_reference() {
        assert_eq!(encode_secret(b"Hello!\xde\xad\xbe\xef"), "JBSWY3DPEHPK3PXP");
        assert_eq!(encode_secret(b"Hello!"), "JBSWY3DPEE");
        assert_eq!(encode_secret(b"\xde\xad\xbe\xef"), "32W353Y");
    }

    #[test]
    fn test_decode_base64_payloads_matches_python_reference() {
        let data = vec![
            "CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC".to_string(),
        ];
        let result = decode_base64_payloads(&data).unwrap();
        assert_eq!(
            result,
            vec![b"\n1\n\nHello!\xde\xad\xbe\xef\x12\x18Example:alice@google.com\x1a\x07Example0\x02".to_vec()]
        );
    }

    #[test]
    fn test_decode_base64_payloads_multiple() {
        let item =
            "CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC".to_string();
        let data = vec![item.clone(), item];
        let result = decode_base64_payloads(&data).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_decode_migration_uri_multiple_data_params_and_plus_as_space() {
        // Payload cuja codificação Base64 contém '+' (ex.: bytes que produzem '+')
        let payload = Payload {
            otp_parameters: vec![OtpParameters {
                secret: vec![0xfb, 0xfb, 0xfb, 0xfb, 0xfb, 0xfb],
                name: "Plus:test".to_string(),
                issuer: "Plus".to_string(),
                algorithm: ProtoAlgorithm::Sha1 as i32,
                digits: ProtoDigitCount::Six as i32,
                r#type: ProtoOtpType::Totp as i32,
                counter: 0,
            }],
            version: 1,
            batch_size: 1,
            batch_index: 0,
            batch_id: 1,
        };
        let mut bytes = Vec::new();
        payload.encode(&mut bytes).unwrap();
        let b64 = STANDARD.encode(bytes);
        assert!(b64.contains('+'), "payload de teste deve conter '+'");

        let uri = format!(
            "otpauth-migration://offline?data={b64}&data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC"
        );
        let accounts = decode_migration_uri(&uri).unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].label, "Plus:test");
        assert_eq!(accounts[1].label, "Example:alice@google.com");
    }

    #[test]
    fn test_decode_migration_uri_rejects_empty_accounts_and_missing_secret() {
        let empty_payload = Payload::default();
        let mut bytes = Vec::new();
        empty_payload.encode(&mut bytes).unwrap();
        let empty_uri = format!(
            "otpauth-migration://offline?data={}",
            STANDARD.encode(bytes)
        );
        assert!(decode_migration_uri(&empty_uri).is_err());

        let no_secret_otp = OtpParameters {
            secret: Vec::new(),
            name: "NoSecret".to_string(),
            issuer: "Test".to_string(),
            algorithm: 0,
            digits: 0,
            r#type: ProtoOtpType::Totp as i32,
            counter: 0,
        };
        assert!(otp_parameters_to_account(&no_secret_otp).is_err());

        let unspecified_type_otp = OtpParameters {
            secret: b"Hello!".to_vec(),
            name: "Unspecified".to_string(),
            issuer: "Test".to_string(),
            algorithm: 0,
            digits: 0,
            r#type: ProtoOtpType::Unspecified as i32,
            counter: 0,
        };
        assert!(otp_parameters_to_account(&unspecified_type_otp).is_err());
    }

    #[test]
    fn test_decode_migration_uri_rejects_invalid_base64_and_protobuf() {
        assert!(decode_migration_uri("otpauth-migration://offline?data=***invalid***").is_err());
        // Base64 válido ("////"), mas Protobuf inválido (varint truncado)
        assert!(decode_migration_uri("otpauth-migration://offline?data=////").is_err());
    }

    #[test]
    fn test_build_url_params_matches_python_reference() {
        let otp = OtpParameters {
            secret: b"Hello!\xde\xad\xbe\xef".to_vec(),
            name: "Example:alice@google.com".to_string(),
            issuer: "Example".to_string(),
            algorithm: 0,
            digits: 0,
            r#type: ProtoOtpType::Totp as i32,
            counter: 0,
        };
        assert_eq!(
            build_url_params(&otp),
            "issuer=Example&secret=JBSWY3DPEHPK3PXP"
        );
    }

    #[test]
    fn test_build_otpauth_url_matches_python_reference() {
        let otp = OtpParameters {
            secret: b"Hello!\xde\xad\xbe\xef".to_vec(),
            name: "Example:alice@google.com".to_string(),
            issuer: "Example".to_string(),
            algorithm: 0,
            digits: 0,
            r#type: ProtoOtpType::Totp as i32,
            counter: 0,
        };
        assert_eq!(
            build_otpauth_url(&otp),
            "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP"
        );
    }

    #[test]
    fn test_validate_migration_uri_valid() {
        let result = validate_migration_uri(EXAMPLE_MIGRATION).unwrap();
        assert_eq!(
            result,
            vec!["CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC"]
        );
    }

    #[test]
    fn test_validate_migration_uri_invalid_cases() {
        let invalid_cases = [
            "otpauth-migration://online?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC",
            "CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC",
            "offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC",
            "otpauth-migration://online?data=Cu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC",
            "data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC",
            "otpauth-migration://offline?data=",
            "otpauth-migration://offline",
        ];

        for broken in invalid_cases {
            assert!(
                validate_migration_uri(broken).is_err(),
                "esperava erro para: {broken}"
            );
        }
    }

    #[test]
    fn test_decode_migration_uri_full_flow() {
        let accounts = decode_migration_uri(EXAMPLE_MIGRATION).unwrap();
        assert_eq!(accounts.len(), 1);
        let account = &accounts[0];
        assert_eq!(account.otp_type, OtpType::Totp);
        assert_eq!(account.label, "Example:alice@google.com");
        assert_eq!(account.issuer.as_deref(), Some("Example"));
        assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
        assert_eq!(
            account.uri,
            "otpauth://totp/Example%3Aalice%40google.com?issuer=Example&secret=JBSWY3DPEHPK3PXP"
        );
    }

    #[test]
    fn test_hotp_includes_counter() {
        let otp = OtpParameters {
            secret: b"Hello!".to_vec(),
            name: "Acme:bob".to_string(),
            issuer: "Acme".to_string(),
            algorithm: ProtoAlgorithm::Sha256 as i32,
            digits: ProtoDigitCount::Eight as i32,
            r#type: ProtoOtpType::Hotp as i32,
            counter: 42,
        };
        let account = otp_parameters_to_account(&otp).unwrap();
        assert_eq!(account.otp_type, OtpType::Hotp);
        assert_eq!(account.counter, Some(42));
        assert_eq!(account.algorithm, Some(Algorithm::Sha256));
        assert_eq!(account.digits, Some(8));
        assert!(account.uri.starts_with("otpauth://hotp/"));
        assert!(account.uri.contains("counter=42"));
    }
}
