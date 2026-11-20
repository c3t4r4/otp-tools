# Recuperação de Contexto (RAG)

## Ordem padrão de recuperação

Antes de leitura ampla de código, seguir esta ordem:

1. `docs/RegrasNegocio.md` — comportamento, validações e regras de negócio.
2. Memória do projeto (`ai-memory` via `memory_query`) e `docs/Memoria.md` — decisões técnicas e armadilhas conhecidas.
3. Busca direcionada nos módulos em `src/` (`migration.rs`, `otpauth.rs`, `qrcode.rs`, `cli.rs`, `ui.rs`).
4. Leitura completa de arquivos apenas nos arquivos identificados.

## Exclusões obrigatórias de segurança

Nunca indexar ou registrar em memória/documentação:

- Segredos TOTP/HOTP reais de usuários (`secret=...`, links `otpauth://` ou `otpauth-migration://` reais).
- Imagens de QR Code pessoais (ex.: `qrcode.png` local contendo contas reais).
- Arquivos `.env` e variantes, chaves privadas, tokens ou logs de execução com segredos.

## Histórico

- `2026-11-19`: Criação inicial da política de recuperação de contexto e exclusões de segurança.
