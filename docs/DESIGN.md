# Portward — Design System

Guia do visual e do comportamento da interface do Portward: como foi feito, por
que, e como manter o mesmo padrão ao criar telas novas.

Os valores aqui são os mesmos do código. Fonte da verdade:
[`ui/dist/styles.css`](../ui/dist/styles.css) (tokens e componentes),
[`ui/dist/app.js`](../ui/dist/app.js) (telas e comportamento) e
[`ui/dist/index.html`](../ui/dist/index.html) (esqueleto, marca e abertura).

---

## 1. Princípios

1. **Escuro, denso e calmo.** Fundo quase preto, cinzas em degraus pequenos,
   cor só onde ela carrega significado. Inspirado no Portmaster e no clima de
   terminal/hacker de Mr. Robot.
2. **Cor é informação, não decoração.** Cada cor tem um papel fixo (veja §2.2).
   Se algo é colorido, o usuário deve saber o que aquilo quer dizer.
3. **Dados primeiro.** Números com algarismos tabulares, IPs e domínios em
   fonte mono e selecionáveis, detalhes atrás de um clique (linhas que expandem,
   painéis, popups).
4. **Nativo de Mac.** Barra de título sobreposta com os semáforos do macOS,
   fonte do sistema, ícone de menu monocromático, nada de logo dentro da
   barra lateral.
5. **Ao vivo sem atrapalhar.** A tela se atualiza sozinha, mas nunca enquanto o
   usuário está arrastando, rolando ou lendo um detalhe aberto.
6. **Marca com personalidade, uso contido.** O neon vermelho e as letras retrô
   aparecem na abertura, no ícone e em poucos acentos. O resto da interface é
   sóbrio.

---

## 2. Tokens

### 2.1 Superfícies e texto

| Token | Valor | Uso |
|---|---|---|
| `--bg` | `#121213` | Fundo da janela e do conteúdo |
| `--g100` | `#131111` | Barra lateral, coluna da lista de apps, campos de texto |
| `--g200` | `#1b1b1b` | Cartões (`.card`, `.idcard`, linhas de conexão) |
| `--g300` | `#222222` | Bordas padrão, divisórias, trilho de barras |
| `--g400` | `#2c2c2c` | Hover forte, item selecionado, pílula neutra |
| `--g500` | `#474747` | Bordas de botões e controles, trilhos de progresso |
| `--g600` | `#888888` | Texto secundário, rótulos, legendas |
| `--g700` | `#ababab` | Texto de apoio, itens de menu inativos |
| `--text` | `#e8e8e8` | Texto principal |
| branco | `#ffffff` | Item ativo, títulos em destaque |

A escala de cinzas sobe em degraus pequenos de propósito: a hierarquia vem de
**borda + leve mudança de fundo**, não de sombras.

### 2.2 Cores com significado

| Cor | Valor | Significa | Onde |
|---|---|---|---|
| Vermelho neon (marca) | `#ff2a36` | Identidade, "você está aqui" | P da marca, item ativo do menu, marcador no mapa |
| Vermelho rota | `#ff5a63` (40% opac.) | Para onde suas conexões vão | Linhas e pontos de destino no mapa |
| Vermelho erro | `--red-soft #ff6b6b` sobre `--red-bg #3d1414` | Bloqueado, erro, ação destrutiva | Pílula BLOQUEADO, botão "Bloquear", caixas de erro |
| Azul | `--blue #4e97fa` (fundo `#16294a`) | Ação primária, Wi‑Fi, enviado, modo monitor | Botão primário, pílula Wi‑Fi, área "Enviado" do gráfico |
| Verde | `--green #1de966` (fundo `--green-bg #143d24`) | Ativo, permitido, recebido | Ponto de conexão ativa, pílula OK, área "Recebido" |
| Roxo | `#b39ddb` (fundo `#2a1d4a`, borda `#3f2f6e`) | VPN / túnel | Pílula VPN, cartões de identidade VPN, rotas e marcador VPN |
| Amarelo | `--yellow #e9d81d` (fundo `--yellow-bg #3d3a14`) | Atenção, pausado | Avisos, herói "Proteção pausada" |

Regra: **toda cor de destaque vem com um fundo escuro da mesma família**
(ex.: texto `#b39ddb` em fundo `#2a1d4a`). Nunca cor saturada sobre cinza puro
em áreas grandes.

### 2.3 Tipografia

| Papel | Fonte | Tamanho / peso |
|---|---|---|
| Base | `-apple-system, "SF Pro Text", Inter` | 13 px |
| Título de página (`h1`) | sistema | 22 px / 650 |
| Título do app no detalhe | sistema | 24 px / 650 |
| Título de cartão (`h2`) | sistema | 14 px / 600 |
| Valor de estatística | sistema | 26 px / 650, `tabular-nums` |
| Rótulo de estatística / categoria | sistema | 11.5–12 px, MAIÚSCULAS, `letter-spacing .5–.6px`, `--g600` |
| Dados técnicos (IP, domínio, porta, hex) | `"SF Mono", ui-monospace, Menlo` | 11.5–12.5 px |
| IP público em destaque | mono | 17 px / 600 |

- Números que mudam ao vivo usam `font-variant-numeric: tabular-nums` para não
  "dançar".
- A interface toda é `user-select: none`; IPs, domínios, caminhos e hex têm
  `.selectable` para poder copiar.
- IPv6 longo é abreviado (`2001:db8:…:8f8c`) com o endereço completo no `title`.

### 2.4 Espaço, raios e bordas

| Item | Valor |
|---|---|
| Padding do conteúdo | 34 px topo, 32 px laterais |
| Gap entre cartões | 14 px (grids), 10 px (identidades, linhas) |
| Padding de cartão | 16–18 px |
| Raio padrão (`--radius`) | 10 px |
| Raio de cartões de identidade / gráfico / mapa | 12 px |
| Raio do herói | 14 px |
| Raio de botões, itens de menu | 7 px |
| Raio de pílulas | 20 px (cápsula) |
| Raio de ícone de app | 8 px (30 px), 14 px (56 px), 5 px (20 px) |
| Borda | sempre 1 px, `--g300` (padrão) ou `--g500` (controles, item aberto) |

---

## 3. Layout da janela

```
┌──────────────────────────────────────────────────────────────┐
│ ● ● ●   (barra de título sobreposta, 28 px, arrastável)       │
├────────────┬─────────────────────────────────────────────────┤
│ Painel     │                                                 │
│ Quem vê    │   conteúdo da página                            │
│ Atividade  │   (rola sozinho; a lista/detalhe da Atividade   │
│ DNS        │    ocupa a altura toda em duas colunas)          │
│ Listas     │                                                 │
│ Regras     │                                                 │
│ Config.    │                                                 │
│            │                                                 │
│ [status]   │                                                 │
└────────────┴─────────────────────────────────────────────────┘
   216 px
```

- **Janela:** 1240×800, mínimo 980×620, `titleBarStyle: Overlay` com título
  oculto. O fundo da janela é `#121213` para não piscar branco ao abrir.
- **Arrastar a janela:** `.titlebar`, a barra lateral e o espaço vazio do
  conteúdo têm `data-tauri-drag-region` (exige a permissão
  `core:window:allow-start-dragging` na capability do Tauri).
- **Barra lateral (216 px, `--g100`):** sem logo nem nome (padrão dos apps de
  Mac). Topo com 46 px de respiro para os semáforos. Itens com ícone de 17 px +
  texto; ativo = fundo `--g300`, texto branco, ícone vermelho neon com brilho
  suave. No rodapé, o cartão de status (modo, interfaces observadas, versão) e,
  quando houver, o banner **"Nova versão pronta · Instalar atualização"**.
- **Atividade de rede:** grade de duas colunas que sangra até as bordas
  (`.split2`): lista de apps com 310 px em `--g100` à esquerda e detalhe
  rolável à direita.

---

## 4. Componentes

### 4.1 Herói (`.hero`)
Faixa no topo do Painel: círculo de 64 px com ícone, título 20 px, frase de
contexto e uma ação à direita. O fundo é um gradiente de 120° da cor do estado
para `--g200`:

| Estado | Gradiente / círculo |
|---|---|
| Monitor | `#142338` → cinza, círculo azul (olho) |
| Protegido | `#17221a` → cinza, círculo verde (escudo com check) |
| Pausado | `#2b2914` → cinza, círculo amarelo |
| Parado | `#2a1515` → cinza, círculo vermelho |

### 4.2 Cartões
- **`.card`**: `--g200`, borda `--g300`, raio 10.
- **Estatística (`.stat`)**: rótulo em maiúsculas pequeno, valor grande
  tabular, dica embaixo em `--g600`. Sempre em grade de 4 (`.g4`).
- **Identidade (`.idcard`)**: pílula do tipo (Wi‑Fi / VPN), nome da interface,
  IP público grande em mono, IPv6 menor, país + operadora, IP local em rodapé.
  A versão VPN ganha borda e gradiente roxos.
- **Identidade compacta (`.ident`)**: usada no detalhe do app. Bandeira + IP
  curto, "N conexões" e a pílula da interface. IPv6 de privacidade antigos
  aparecem como "IPv6 temporário anterior".

### 4.3 Pílulas (`.pill`)
Cápsula de 11 px, peso 600, `letter-spacing .3px`, sempre cor + fundo escuro da
mesma família: `block`, `ok`, `gray`, `blue`, `wifi`, `vpn`.

### 4.4 Botões
- Padrão: fundo `--g300`, borda `--g500`, raio 7; hover `--g400`;
  `:active` escala 0.97.
- `primary`: azul cheio. `danger`: contorno vermelho, vira fundo vermelho
  escuro no hover. `ok`: o mesmo em verde. `small`: 3×8 px de padding, 12 px.
- Botões de ação em linhas de tabela só aparecem no hover da linha.
- **Botão do mapa (`.mapbtn`)**: 28×28, fundo translúcido escuro, ícone de 15 px
  centralizado (`display: block; margin: auto`). Atenção: regras genéricas como
  `.mapcard svg` não podem atingir o ícone. Use `.mapcard > svg`.

### 4.5 Segmentado (`.seg`) e chips
- `.seg`: grupo de botões sem bordas internas; ativo = `--g400`. Usado em
  "Por servidor · Por app · Pacotes".
- `.chip`: filtro ativo removível ("App: Discord ✕"), azul escuro `#1d2a40`.

### 4.6 Lista de apps (Atividade de rede)
Linha com ícone real do app (30 px), nome e uma **barra de conexões**
(`.cbar`): número tabular + trilho de 4 px com preenchimento verde proporcional
ao app com mais conexões. Selecionado = `--g400`.

### 4.7 Detalhe do app
De cima para baixo:
1. Trilha "Apps › Nome" em 12 px.
2. Cabeçalho: ícone de 56 px, nome 24 px, linhas `Caminho / Binário / Conexões`
   (rótulo `--g600`, valor mono), botão "Mostrar no Finder".
3. **Gráfico de tráfego** (§5.1).
4. **Identidades**: "N identidades · o IP que os servidores deste app veem".
5. Aba "Conexões" sublinhada em azul + contagem.
6. **Linhas de conexão** que expandem (§4.8).

### 4.8 Linha de conexão expansível (`.crow`)
Cabeçalho em grade fixa:
`chevron · ponto ativo · domínio · país · ↗ IP remoto · estado · bytes · VPN · ação`.
Ao abrir, a borda passa para `--g500` e mostra:
- **Grade de 3 colunas** chave/valor: tempo e processo · direção, protocolo,
  criptografia, túnel, tráfego · domínio, servidor, país, ASN, empresa.
- **Rota** (`.route`): linha do tempo vertical com nós de 12 px:
  `Seu dispositivo` (anel cinza) → `Saída para a internet` (azul, ou roxo se
  VPN, com o IP que o servidor vê) → `Destino` (verde).

O estado aberto é guardado por id de conexão e sobrevive às atualizações.

### 4.9 Tabelas
`--g200` com bordas `--g300`, cabeçalho grudado no topo, hover `#1f1f1f`.
Células de dados não quebram linha (`.table-wrap td`). Tabelas de texto
explicativo (`.syntax`) quebram normalmente. Nunca aplique `nowrap` em
todas as `td` globalmente.

### 4.10 Pacotes (estilo Wireshark)
- Lista em mono 11.5 px com colunas `Nº · Tempo relativo · → / ← · Seu endereço
  · Servidor · Proto · Tam. · App · Info`.
- Cores por tipo na coluna Info: TLS amarelo `#e9d98a`, DNS azul claro
  `#9fd3ff`, SYN lilás `#c7a6ff`, RST vermelho. Direção: enviado azul claro,
  recebido verde claro.
- Clicar num pacote **pausa a lista** e abre o painel de detalhe: campos à
  esquerda (300 px) e hexdump à direita (offset em `--g500`, 16 bytes por linha,
  coluna ASCII).

### 4.11 Avisos
- `.warn`: amarelo escuro, para condições do sistema (sem root etc.).
- `.filter-only`: borda tracejada, para "isto só vale no modo bloqueio".
- `.errorbox`: vermelho escuro, para erros de validação.
- `#toast`: cápsula no rodapé central, some em 2,6 s; variante `err`.

---

## 5. Visualização de dados

### 5.1 Gráfico de tráfego
- Área dupla em SVG, 1000×130 com `preserveAspectRatio="none"`, últimos
  30 minutos em baldes de 10 s.
- **Recebido = verde `#1de966`**, **Enviado = azul `#4e97fa`**, cada um com
  gradiente vertical (45–50% → 2–3% de opacidade) e contorno de 1.2 px com
  `vector-effect: non-scaling-stroke`.
- Suavização por média móvel de 5 pontos para silhueta macia (como no
  Portmaster).
- Fundo do cartão: gradiente `#151a17 → --g200`. Legenda com totais e pico/s.

### 5.2 Listas de ranking (`.toplist`)
Barra horizontal atrás do texto (largura proporcional ao máximo), número
tabular à direita. Variante `red` para bloqueados. Países usam bandeira emoji
(gerada do código ISO) + nome.

### 5.3 Mapa
- **Motor:** MapLibre GL JS (BSD) com o estilo vetorial `dark` do OpenFreeMap
  (grátis, sem chave). Fallback offline: SVG gerado do Natural Earth 1:50m em
  `world.js`.
- **Uma instância por sessão:** o mapa vive num nó próprio (`.maphost`) que é
  movido para o `.mapslot` da página atual. Redesenhar a página não recria o
  mapa nem perde zoom.
- **Zoom inicial ~2.6** centrado em você (mostra o país). Zoom e arrasto pelo
  usuário.
- **Você:** marcador vermelho neon de 14 px, borda branca, brilho e um halo que
  pulsa. VPN = roxo.
- **Rotas:** grandes círculos (48 pontos) da sua saída até cada cidade de
  destino, em `#ff5a63` com 40% de opacidade e espessura por `log2(conexões)`,
  mais uma camada de brilho borrado a 12%. Ficam **mais fracas que o seu
  marcador** de propósito.
- **Hover numa rota ou destino:** a linha acende (95% de opacidade, mais
  grossa), e um popup escuro (`.pw-pop`) mostra origem → destino, empresa,
  número de conexões, apps e os domínios daquela rota.
- **Sem botão "i" de atribuição** no cartão. O crédito obrigatório
  (OpenFreeMap · OpenMapTiles · OpenStreetMap) fica no cabeçalho do mapa
  expandido.
- **Expandir:** botão no canto superior direito abre o mapa em sobreposição
  (fundo 60% preto, painel com raio 14). Esc, ✕ ou clique fora fecham.

---

## 6. Identidade da marca

### 6.1 Conceito
Inspirado na estética da série Mr. Robot: preto, branco e um vermelho neon,
letras retrô‑futuristas de traço duplo ("inline"). **Tudo é original.** Não use o
logo nem a máscara da fsociety, que são marcas da série.

### 6.2 Ícone do app — globo neon
Gerado por [`scripts/make_icon.py`](../scripts/make_icon.py) (Python puro, sem
dependências), a partir de funções de distância:

- Quadrado arredondado preto (`#0a0a0c`, raio 0.38 do lado).
- **Anel vermelho** `#ea2027` (raio 0.60).
- **Grade branca** `#f5f5f5` que para antes do anel: um meridiano elíptico
  (55% da largura), o eixo central e três paralelos (−50%, 0, +50%).
- Estilo **inline**: cada traço tem 0.074 de espessura com um sulco escuro de
  0.020 no meio.
- **Brilho**: halo exponencial (decai em 0.035) da cor de cada traço, mais um
  leve brilho vermelho atrás do anel.

```bash
python3 scripts/make_icon.py ui/src-tauri/icons/source.png 1024
```

```bash
cd ui && npx @tauri-apps/cli@2 icon src-tauri/icons/source.png -o src-tauri/icons
```

### 6.3 Ícone da barra de menus
O mesmo globo, só linhas, **preto com alfa** (44×44 px) e marcado como
*template image* (`icon_as_template(true)`). O macOS pinta de branco ou preto
conforme o tema, como todo ícone nativo.

```bash
python3 scripts/make_icon.py ui/src-tauri/icons/tray.png 44 --tray
```

### 6.4 Logotipo "PORTWARD"
Desenhado em SVG à mão, letra por letra, num grid de 100 de altura:

- Cada letra é um **traço central** (`path`) desenhado duas vezes: primeiro com
  `stroke-width 18` na cor, depois com `stroke-width 3.2` na cor do fundo
  (`--groove`). O segundo traço abre o sulco que dá o visual "inline".
- Topo e base das letras em y = 7 e y = 93 (metade do traço = 9). Cantos com
  `stroke-linejoin: round`.
- **P em vermelho neon `#ff2a36`** com brilho duplo
  (`drop-shadow 4px .85` + `10px .45`). Demais letras em `#f4f4f4` com brilho
  branco discreto.
- Espaço de 10 entre letras. Larguras: P 60, O 68, R 62, T 60, W 80, A 66, D 64.

Formas (coordenadas do traço central):

| Letra | Path |
|---|---|
| P | `M9,93 V7 H33 A24,24 0 0 1 33,55 H9` |
| O | `M33,7 H35 A24,24 0 0 1 59,31 V69 A24,24 0 0 1 35,93 H33 A24,24 0 0 1 9,69 V31 A24,24 0 0 1 33,7 Z` |
| R | `P` + `M30,55 L57,93` |
| T | `M3,7 H57 M30,7 V93` |
| W | `M5,7 L22,93 L40,30 L58,93 L75,7` |
| A | `M5,93 L33,7 L61,93 M16,64 H50` |
| D | `M9,7 H31 A26,26 0 0 1 57,33 V67 A26,26 0 0 1 31,93 H9 Z` |

A variável `--groove` precisa ser a cor do fundo onde o logo está
(`--bg` na abertura).

### 6.5 Abertura (`#splash`)
Ícone de 64 px + logotipo de 34 px centralizados sobre `--bg`:
- entrada em **300 ms** (`opacity 0→1`, `scale .94→1`, `blur 3px→0`,
  `cubic-bezier(.2,.8,.2,1)`);
- saída em 160 ms a partir de 400 ms. Total ≈ 0,5 s; o nó é removido em 750 ms;
- com `prefers-reduced-motion`, sem movimento, só um corte rápido.

É o único lugar onde a marca aparece grande. A barra lateral fica sem logo.

---

## 7. Movimento

| O quê | Duração | Curva |
|---|---|---|
| Hover de menu, botões, cores | 120 ms | linear |
| Toque em botão | escala 0.97 | — |
| Interruptor (`.switch`) | 150 ms | — |
| Toast | 180 ms | — |
| Abertura | 300 ms entrada + 160 ms saída | `cubic-bezier(.2,.8,.2,1)` |
| Pulso do marcador | 2,2 s, infinito | ease-out, `scale .4→1.6`, opacidade `.55→0` |

Tudo respeita `prefers-reduced-motion`.

---

## 8. Comportamento ao vivo (importante)

- A página atual é redesenhada a cada **1,5 s** (5 s em Regras, Configurações
  e Listas, que têm formulários).
- **Pausa enquanto o mouse está pressionado** (arrastar barra de rolagem,
  selecionar texto). Retoma 400 ms depois de soltar.
- **Posições de rolagem preservadas**: antes de redesenhar, guarda o
  `scrollTop/Left` de `#main` e de todo elemento com `id`; depois restaura.
  Toda área rolável nova precisa ter `id`.
- Estado de UI (app selecionado, linhas abertas, filtros, pacote selecionado,
  aba) vive em `state`/`expanded`/`pk` e nunca no DOM.
- Formulários (Regras, Configurações) só são desenhados na entrada da página,
  para não apagar o que o usuário digita.

---

## 9. Ícones de apps
Ícone real do Finder via AppKit (`NSWorkspace.iconForFile`, por JXA), desenhado
em 64×64, salvo em `~/Library/Caches/Portward/icons/` e enviado como data URL.
Enquanto carrega, mostra um avatar com a inicial sobre uma cor derivada do nome
(`hsl(hash 45% 38%)`), que é trocado sem piscar quando o ícone chega.

---

## 10. Texto e voz
- Português do Brasil, frases curtas, sem jargão quando dá:
  "Quem vê seu IP", "Seu IP que ele vê", "aproximado, pela localização do seu IP".
- Termos técnicos aparecem só onde o usuário espera (Pacotes, detalhes da
  conexão): SYN, ACK, TLS, ASN.
- Avisos dizem o que acontece e o que fazer ("Rodando sem root: só os seus
  próprios processos aparecem.").
- Contagens com plural correto ("1 conexão", "3 conexões").

---

## 11. Acessibilidade
- Contraste: texto principal `#e8e8e8` sobre `#121213`/`#1b1b1b`; secundário
  `#888` só para rótulos e metadados.
- Toda informação de cor também tem texto (pílula "VPN", "BLOQUEADO", seta de
  direção, legenda do gráfico).
- `prefers-reduced-motion` desliga a animação de abertura e reduz transições.
- Controles têm `title` descritivo (ex.: "Expandir mapa", "Fechar (Esc)").

---

## 12. Não fazer
- Logo ou nome do app dentro da barra lateral.
- Cor de destaque sem fundo da mesma família.
- `white-space: nowrap` global em células de tabela.
- Seletores genéricos (`.cartão svg`) que vazam para ícones de botões.
- Recriar o mapa a cada atualização (perde zoom e pisca).
- Redesenhar formulários enquanto o usuário digita.
- Controles de terceiros com aparência clara (popups, atribuição) sem
  restilizar no tema escuro.
- Usar o logo, a máscara ou o nome da fsociety.
