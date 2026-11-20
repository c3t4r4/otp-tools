# Harness Multiagente

## Papéis obrigatórios

Toda tarefa não trivial segue o fluxo multiagente definido em `~/.gemini/config/rules/AGENTS.md`:

| Papel               | Modelo              | Neste projeto                                                                                  |
| ------------------- | ------------------- | ---------------------------------------------------------------------------------------------- |
| Planner             | `pro` (ou `inherit`) | `inline` na conversa principal (análise, riscos e plano em `.agents/plans/`).                  |
| Coder               | `flash`             | `inline` ou subagente `self` (`Model: "flash"`) para implementação, testes e documentação.     |
| Validator           | `pro` (ou `inherit`) | subagente `research` (`Model: "pro"`) para revisão de arquitetura, regras e sincronização.     |
| Tester              | `flash`             | subagente `self` (`Model: "flash"`) para `cargo test`, `cargo clippy`, `rustfmt` e testes CLI. |
| Security Specialist | `pro` (ou `inherit`) | subagente `research` (`Model: "pro"`) para auditoria de segurança (imagens, ANSI, segredos).   |

## Tabela de gatilhos de documentação

| Documento                | Gatilho                                                                                                                                                                     | Sem gatilho             |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| `docs/RegrasNegocio.md`  | comportamento, validação, permissão, máquina de estado ou regra de cálculo mudou                                                                                            | `sem alteração`         |
| `docs/Arquitetura.md`    | camada, módulo, padrão, dependência, fluxo de dados, schema ou decisão arquitetural mudou — regenerar também `docs/arquitetura.html` via skill `archify`, quando disponível | `sem alteração`         |
| `docs/Organograma.md`    | módulo, camada, fluxo de negócio, tela, integração externa ou relação entre componentes mudou de forma que o diagrama fique desatualizado                                   | `sem alteração`         |
| `docs/Infraestrutura.md` | Docker, Compose, deploy, rede, volume, variável de ambiente, build ou observabilidade mudou                                                                                 | `sem alteração` / `n/a` |
| `docs/API.md`            | rota, método, payload, header, código HTTP, autenticação de endpoint, paginação ou filtro mudou                                                                             | `sem alteração` / `n/a` |
| `docs/Frontend.md`       | componente, tela, rota de UI, token de design, padrão de estado ou acessibilidade mudou                                                                                     | `sem alteração` / `n/a` |
| `docs/Auth.md`           | authn, authz, sessão, token, papel, permissão ou política de senha mudou                                                                                                    | `sem alteração` / `n/a` |
| `docs/RAG.md`            | fonte indexada, corpus, exclusão de segurança ou capacidade do ambiente mudou                                                                                               | `sem alteração`         |
| `docs/Progresso.md`      | **sempre** — toda tarefa concluída gera uma linha                                                                                                                           | nunca `sem alteração`   |
| `docs/Memoria.md`        | aprendizado não óbvio, armadilha de ambiente, comando descoberto ou decisão com motivo                                                                                      | `sem alteração`         |
| `docs/Harness.md`        | papel, agente, fluxo de aprovação ou ferramenta do harness mudou                                                                                                            | `sem alteração`         |
| `README.md`              | nome, propósito, instruções de instalação/uso, stack, comandos principais, configuração ou licença do projeto mudou                                                         | `sem alteração`         |

Neste projeto CLI + GUI local: `docs/API.md` e `docs/Auth.md` são `n/a` enquanto não houver API HTTP ou autenticação de usuários. `docs/Infraestrutura.md` e `docs/Frontend.md` estão ativos.

## Histórico

- `2026-11-19`: Criação inicial da documentação do harness multiagente, com mapeamento de papéis e tabela de gatilhos.
- `2026-11-19`: Atualização do estado dos documentos condicionais após criação de `docs/Infraestrutura.md` e `docs/Frontend.md`.
