# Frontend / Interface Gráfica (GUI)

## Visão geral

A interface gráfica desktop do **OTP Tools** é implementada em Rust puro no binário `otp-tools-gui` (`src/bin/otp-tools-gui.rs` + `src/gui.rs`) usando `eframe` / `egui`, `rfd` (seletor nativo de arquivos) e `qrcode` (geração de QR Code por conta), carregando `assets/AppIcon.ico` como ícone de janela em runtime via `ViewportBuilder::with_icon`.

## Telas e abas

- **Navegação por Sidebar Lateral (`SidePanel::left("sidebar_nav")`, `230 px`)**:
  - Topo com emblema squircle (`OTP`) e título **OTP Tools**.
  - Menu vertical com itens em pílula arredondada (`sidebar_nav_item`) e ícones em squircles roxo-violeta azulado/índigo (`palette.accent_fill` / `palette.secondary_badge`): **Ler QR Code (`T-01`)**, **Migration (`T-02`)** (com setas bidirecionais desenhadas vetorialmente no `painter`), **Analisar OTP (`T-03`)** e **Sobre (`T-04`)**.
  - Base da sidebar com controles rápidos em largura total: **Mostrar/Ocultar Secrets** (ao ocultar, também limpa `qr_textures`), **Tema Claro/Escuro** e **Limpar tela**.
- **`T-01` — Ler QR Code (`GuiTab::QrCode`)**:
  - Cabeçalho Hero (**"Leitura Inteligente de QR Code"**) com 3 `feature_bullet` em squircles índigo-violeta.
  - Suporta **Drag & Drop** (arrastar e soltar imagens `PNG`, `JPG` ou `JPEG` diretamente sobre a janela, com overlay visual de destaque), botão primário **"Selecionar imagem QR..."** usando o diálogo nativo (`rfd::FileDialog`) e botão **"Usar Câmera"** / **"Parar Câmera"** integrados dentro da mesma `ScrollArea` vertical dos resultados.
  - Quando a câmera está ativa, exibe logo abaixo um card **Viewfinder da Câmera** (`render_camera_viewfinder`) com badge **CÂMERA AO VIVO**, resolução detectada (`WxH`), quadro de pré-visualização em tempo real com cantoneiras guia (retículo de mira) e botão **Parar Câmera**; ao decodificar qualquer QR Code, desliga a câmera automaticamente e limpa a textura de vídeo.
  - Trocar de aba, carregar/soltar um arquivo, clicar em **Limpar tela** ou fechar o aplicativo também encerra imediatamente a sessão da câmera e zeroiza os quadros em memória.
  - Exibe resultados classificados:
    - `otpauth://totp/` ou `otpauth://hotp/`: card Liquid Glass responsivo dividido em **3 partes** (**2/3** para dados em 2 subcolunas de tiles + link completo somente leitura + botões **Copiar Secret** / **Copiar Link otpauth://**; e **1/3** para o pod do **QR Code OTP** gerado dinamicamente). Em janelas estreitas (`< 560 px` internos), empilha os blocos verticalmente sem corte.
    - `otpauth-migration://`: badge com total de contas, botão **Copiar todos os links** e lista rolável de cards individuais (cada um com seu próprio QR Code).
    - **QR Code inválido**: card com borda vermelha, badge de erro, motivo, caixa somente-leitura com o conteúdo bruto sanitizado e botão **Copiar conteúdo**.
- **`T-02` — Decodificar `otpauth-migration://` (`GuiTab::DecodeMigration`)**:
  - Cabeçalho Hero + card com `TextEdit::singleline` (margem interna `16x12`, alinhamento central vertical/horizontal e máscara por padrão) para colar o link de migração + botões **Decodificar** e **Limpar**.
- **`T-03` — Analisar `otpauth://` (`GuiTab::ParseOtpAuth`)**:
  - Cabeçalho Hero + card com `TextEdit::singleline` (margem interna `16x12`, alinhamento central vertical/horizontal e máscara por padrão) para colar um link `otpauth://totp/...` ou `otpauth://hotp/...` + botões **Analisar** e **Limpar**.
- **`T-04` — Sobre (`GuiTab::About`)**:
  - Cabeçalho Hero + card Liquid Glass exibindo **OTP Tools**, versão, descrição offline, criador (**Glauco G. Cetara**), e-mail (`glaucogcetara@gmail.com` com link `mailto:` + botão copiar), link clicável do GitHub (`https://github.com/c3t4r4/otp-tools.git` + botão copiar) e **Licença MIT**.

## Design System, componentes e estado

- **Tema Royal Purple / Indigo-Violet Glass (`paint_background_gradient`, `apply_modern_theme` / `ThemePalette`)**:
  - Fundo desenhado com gradiente Mesh de 4 vértices na camada `background` (`#17083A` → `#3E1076` → `#6818A5` no Dark Mode; lavanda/violeta suave `#F3E8FF` → `#E9E5FF` → `#E0DAFE` no Light Mode).
  - Botões primários, pílulas e acentos em **roxo-violeta azulado** (`accent_fill = #6D43F6` e `accent = #8B68FF` no Dark Mode; `accent_fill = #6348EE` e `accent = #4F39DA` no Light Mode), eliminando o tom magenta/rosa nos botões de ação.
  - Cards translúcidos (`card_bg`, margem interna `24x22` px compensada por `CARD_FRAME_CHROME_W = 50.0`), tiles internos (`well_bg`), `item_spacing = 12x12` px, `button_padding = 16x9` px, espaçamento vertical dedicado (`ui.add_space`) separando textos auxiliares e botões em `T-01`/`T-02`/`T-03` e nos cards de conta, bordas especulares sutis (`card_stroke`, `well_stroke`), sombras (`card_shadow`) e cantos arredondados (`CornerRadius::same(18)` nos cards, `14` no pod QR, `12` nos tiles e itens ativos da sidebar, `10` nos botões, `99` em badges).
  - Todas as **badges** (`v0.1.0`, `MIT License`, `TOTP`/`HOTP`, `1/N`, `CÂMERA AO VIVO`) usam preenchimento sólido vibrante (`accent_fill`, `secondary_badge`, `success`, `danger`) com **texto branco puro (`Color32::WHITE`)** para alto contraste.
- **Nitidez tipográfica e anti-aliasing (`configure_crisp_fonts`)**:
  - Carrega exclusivamente **fontes estáticas** (`fvar=False`) do sistema (`HelveticaNeue.ttc` / `Arial.ttf` para proporcional e `Menlo.ttc` / `Monaco.ttf` para monoespaçada no macOS; `segoeui.ttf` / `consola.ttf` no Windows; `DejaVuSans.ttf` / `DejaVuSansMono.ttf` no Linux), evitando a colisão de glifos causada por fontes variáveis `SFNS` sem tabela `HVAR` no `ab_glyph`.
  - Mantém `tessellation_options.feathering = true` para curvas vetoriais suaves sem serrilhado em botões, pílulas e cards.
- **Estado isolado e testável (`GuiState`)**: toda lógica de processamento (`process_qr_path`, `process_camera_classifications`, `process_camera_error`, `process_migration`, `process_otpauth`, `toggle_qr_visibility`, `clear_all`), mascaramento (`mask_secret`, `mask_otpauth_uri_secret`), carregamento de ícone (`load_app_icon`), fontes (`configure_crisp_fonts`) e geração de QR (`generate_qr_color_image`) possui testes unitários sem acoplamento a GPU/display.
- **Privacidade e segurança por padrão**:
  - `reveal_secrets` inicia como `false` — tanto o campo `Secret (Base32)` quanto o parâmetro `secret=` no campo somente-leitura **Link completo** (mascarado em ASCII `*`, sem `%E2%80%A2`), os campos de entrada e o **QR Code de cada conta** iniciam ocultos até ação explícita (**Mostrar QR** individual por conta ou **Mostrar Secrets** global).
  - Campos de resultado (**Link completo** e **Conteúdo encontrado no QR Code**) são renderizados como somente leitura (`&str` imutável em `egui::TextEdit`) com largura finita calculada, mantendo seleção/cópia sem edição e sem estouro horizontal.
  - Botão **Limpar tela** (e os botões **Limpar** das abas e `App::on_exit`) zeroizam buffers sensíveis via `zeroize`, limpam o cache de texturas de QR Code (`qr_textures`), removem o histórico de undo (`TextEditState`) do `egui::Context` e retornam `reveal_secrets` para `false`.
  - Persistência de janela/memória explicitamente desativada (`persist_window: false` e `persist_egui_memory() -> false`).
- **Ergonomia**:
  - Dimensão inicial `1060x740` (mínima `780x540`) com `ScrollArea` vertical única abrangendo cabeçalho Hero, ação/entrada e resultados, e texturas de QR Code em `TextureOptions::NEAREST` para leitura nítida por câmeras de celular.

## Histórico

- `2026-11-19`: Criação inicial da documentação de GUI (`egui`/`eframe`, telas `T-01`..`T-03`, privacidade de segredos e padrão `GuiState`).
- `2026-11-19`: Renomeação para **OTP Tools**, redesign do tema arredondado Dark/Light, QR Code por conta com privacidade e cache de texturas, ícone runtime (`AppIcon.ico`) e adição da tela Sobre (`T-04`).
- `2026-11-19`: Redesign Apple Liquid Glass (Dark/Light), carregamento de fontes nativas (`SFNS`/`SFNSMono`) sem `feathering`, barra de abas em cápsula e layout responsivo em 3 partes (`2/3` dados em 2 subcolunas de tiles + `1/3` QR Code).
- `2026-11-19`: Redesign inspirado no CleanMyMac X com gradiente Mesh Royal Purple/Magenta, Sidebar lateral em pílula (`230 px`), squircles, badges sólidas com texto branco, troca de fontes variáveis para estáticas (`HelveticaNeue.ttc`/`Menlo.ttc`) e restauração de `feathering = true`.
- `2026-11-19`: Inclusão da leitura ao vivo via câmera/webcam na tela `T-01` (botão **Usar Câmera** / **Parar Câmera**, card Viewfinder com cantoneiras de mira e desligamento automático após leitura ou saída da tela).
- `2026-11-20`: Ajuste de espaçamentos/paddings em toda a GUI (`Margin::symmetric(24, 22)`, `item_spacing 12x12`, `button_padding 16x9`, separação vertical entre botões e textos em `T-01`/`T-02`/`T-03`/cards) e troca do acento rosa dos botões para roxo-violeta azulado (`#6D43F6`/`#6348EE`).
- `2026-11-20`: Substituição do glifo `"⇄"` por ícone vetorial no botão **Migration** da sidebar e centralização com padding `16x12` nos inputs de `T-02` e `T-03`.
