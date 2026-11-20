use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use owo_colors::OwoColorize;

use crate::model::{sanitize_terminal_text, OtpAccount, QrClassification};

/// Renderiza uma conta OTP em tabela estruturada + link completo `otpauth://...`.
pub fn render_account(account: &OtpAccount, position: Option<(usize, usize)>) -> String {
    let mut out = String::new();

    if let Some((idx, total)) = position {
        out.push_str(&format!(
            "\n{} {}\n",
            format!("[{idx}/{total}]").bright_cyan().bold(),
            sanitize_terminal_text(&account.label).bold()
        ));
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Campo")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
            Cell::new("Valor")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan),
        ]);

    table.add_row(vec![
        Cell::new("Tipo").add_attribute(Attribute::Bold),
        Cell::new(account.otp_type.display_upper()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("Conta / Label").add_attribute(Attribute::Bold),
        Cell::new(sanitize_terminal_text(&account.label)),
    ]);
    table.add_row(vec![
        Cell::new("Emissor (Issuer)").add_attribute(Attribute::Bold),
        Cell::new(
            account
                .issuer
                .as_deref()
                .map(sanitize_terminal_text)
                .unwrap_or_else(|| "-".to_string()),
        ),
    ]);
    table.add_row(vec![
        Cell::new("Secret (Base32)").add_attribute(Attribute::Bold),
        Cell::new(sanitize_terminal_text(&account.secret))
            .fg(Color::Yellow)
            .add_attribute(Attribute::Bold),
    ]);
    table.add_row(vec![
        Cell::new("Algoritmo").add_attribute(Attribute::Bold),
        Cell::new(
            account
                .algorithm
                .map(|a| a.as_str().to_string())
                .unwrap_or_else(|| "SHA1 (padrão)".to_string()),
        ),
    ]);
    table.add_row(vec![
        Cell::new("Dígitos").add_attribute(Attribute::Bold),
        Cell::new(
            account
                .digits
                .map(|d| d.to_string())
                .unwrap_or_else(|| "6 (padrão)".to_string()),
        ),
    ]);

    if let Some(period) = account.period {
        table.add_row(vec![
            Cell::new("Período (s)").add_attribute(Attribute::Bold),
            Cell::new(period.to_string()),
        ]);
    }

    if let Some(counter) = account.counter {
        table.add_row(vec![
            Cell::new("Contador (HOTP)").add_attribute(Attribute::Bold),
            Cell::new(counter.to_string()),
        ]);
    }

    out.push_str(&table.to_string());
    out.push('\n');
    out.push_str(&format!(
        "{} {}\n",
        "Link completo:".bright_blue().bold(),
        sanitize_terminal_text(&account.uri).underline()
    ));

    out
}

/// Renderiza uma lista de contas vindas de `otpauth-migration://`.
pub fn render_migration_accounts(accounts: &[OtpAccount]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n{} {}\n",
        "✓ Migration decodificado:".bright_green().bold(),
        format!("{} conta(s) encontrada(s)", accounts.len()).bold()
    ));

    let total = accounts.len();
    for (idx, account) in accounts.iter().enumerate() {
        out.push_str(&render_account(account, Some((idx + 1, total))));
    }

    out
}

/// Renderiza o resultado de um QR Code classificado (OTPAuth, Migration ou Inválido).
pub fn render_qr_classification(
    classification: &QrClassification,
    qr_index: usize,
    total_qrs: usize,
) -> String {
    let mut out = String::new();

    if total_qrs > 1 {
        out.push_str(&format!(
            "\n{}\n",
            format!("═══ QR Code {qr_index} de {total_qrs} ═══")
                .bright_magenta()
                .bold()
        ));
    }

    match classification {
        QrClassification::OtpAuth(account) => {
            out.push_str(&format!(
                "\n{} {}\n",
                "✓ QR Code válido:".bright_green().bold(),
                format!("otpauth://{}", account.otp_type.as_str())
                    .cyan()
                    .bold()
            ));
            out.push_str(&render_account(account, None));
        }
        QrClassification::Migration { accounts, .. } => {
            out.push_str(&format!(
                "\n{} {}\n",
                "✓ QR Code válido:".bright_green().bold(),
                "otpauth-migration://".cyan().bold()
            ));
            out.push_str(&render_migration_accounts(accounts));
        }
        QrClassification::Invalid {
            raw_content,
            reason,
        } => {
            out.push_str(&format!(
                "\n{} {}\n",
                "✗ QR Code inválido:".bright_red().bold(),
                sanitize_terminal_text(reason).yellow()
            ));

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![Cell::new("Conteúdo encontrado no QR Code")
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Red)]);
            table.add_row(vec![Cell::new(sanitize_terminal_text(raw_content))]);

            out.push_str(&table.to_string());
            out.push('\n');
        }
    }

    out
}
