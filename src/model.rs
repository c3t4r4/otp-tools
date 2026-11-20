use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtpType {
    Totp,
    Hotp,
}

impl OtpType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Hotp => "hotp",
        }
    }

    pub fn display_upper(self) -> &'static str {
        match self {
            Self::Totp => "TOTP",
            Self::Hotp => "HOTP",
        }
    }
}

impl fmt::Display for OtpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

impl Algorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
            Self::Md5 => "MD5",
        }
    }

    pub fn parse_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "SHA1" | "SHA-1" => Some(Self::Sha1),
            "SHA256" | "SHA-256" => Some(Self::Sha256),
            "SHA512" | "SHA-512" => Some(Self::Sha512),
            "MD5" => Some(Self::Md5),
            _ => None,
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpAccount {
    pub otp_type: OtpType,
    pub label: String,
    pub issuer: Option<String>,
    pub secret: String,
    pub algorithm: Option<Algorithm>,
    pub digits: Option<u32>,
    pub period: Option<u32>,
    pub counter: Option<i64>,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrClassification {
    OtpAuth(OtpAccount),
    Migration {
        raw_uri: String,
        accounts: Vec<OtpAccount>,
    },
    Invalid {
        raw_content: String,
        reason: String,
    },
}

/// Remove ou escapa caracteres de controle que poderiam executar sequências ANSI
/// ou corromper o terminal ao exibir conteúdo arbitrário vindo de um QR Code.
pub fn sanitize_terminal_text(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => sanitized.push(ch),
            '\r' => sanitized.push_str("\\r"),
            c if c.is_control() => {
                sanitized.push_str(&format!("\\u{{{:04X}}}", c as u32));
            }
            c => sanitized.push(c),
        }
    }
    sanitized
}

/// Sanitiza texto para saída em linha única (ex.: modo `--plain`), escapando inclusive `\n`, `\r` e `\t`.
pub fn sanitize_single_line_terminal_text(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            c if c.is_control() => {
                sanitized.push_str(&format!("\\u{{{:04X}}}", c as u32));
            }
            c => sanitized.push(c),
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_text;

    #[test]
    fn sanitize_escapes_ansi_escape_sequence() {
        let malicious = "https://example.com/\x1b[31mFAKE\x1b[0m\x07";
        let safe = sanitize_terminal_text(malicious);
        assert!(!safe.contains('\x1b'));
        assert!(!safe.contains('\x07'));
        assert!(safe.contains("\\u{001B}"));
        assert!(safe.contains("\\u{0007}"));
    }

    #[test]
    fn sanitize_preserves_normal_text_and_newlines() {
        let text = "Linha 1\nLinha\t2";
        assert_eq!(sanitize_terminal_text(text), text);
    }
}
