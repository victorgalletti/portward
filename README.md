# Portward

Filtro de privacidade e monitor de rede para macOS, escrito em Rust. É um clone
do [Portmaster](https://github.com/safing/portmaster) feito para o Mac: bloqueia
anúncios, rastreadores e malware no DNS e mostra, em tempo real, qual app está
falando com qual servidor.

## Modos

- **Monitor (padrão)**: só observa. Não mexe no DNS nem bloqueia nada. Mostra
  quais apps falam com quais servidores, em que país, de que empresa, quanto
  tráfego passou e **qual IP seu cada servidor vê**.
- **Monitor + bloqueio**: tudo do monitor, mais o filtro DNS e as regras abaixo.
  O DNS do Mac passa a apontar para o Portward enquanto o serviço roda.

A troca é feita em Configurações. O serviço reinicia sozinho (o launchd o sobe
de novo).

## Quem vê seu IP

Para cada interface de rede (Wi‑Fi, Ethernet, VPN), o Portward pergunta a um
serviço de eco de IP (`api.ipify.org` / `api6.ipify.org`) *por aquela
interface*. A resposta é o endereço público que o mundo vê por ali. Depois liga
cada conexão à interface por onde ela sai. Com uma VPN em túnel dividido, os
apps no túnel aparecem com o IP da VPN e o resto com o seu IP normal.

Com IPv6, os sites veem o endereço do próprio Mac (não há NAT), e o Portward
mostra isso.

A captura passiva (libpcap, só leitura) acrescenta:
- bytes recebidos e enviados por conexão;
- nomes dos servidores, a partir das respostas DNS que passam pela rede e do
  nome do site no início das conexões HTTPS (SNI). O SNI funciona mesmo quando o
  app usa DNS criptografado próprio.

País e empresa vêm das bases gratuitas DB-IP Lite (CC BY 4.0), baixadas uma vez
por mês e consultadas localmente.

## O que faz (modo bloqueio)

- **Filtro DNS**: servidor DNS próprio em `127.0.0.1:53`. O macOS passa a enviar
  todas as consultas para ele. Domínios em listas de bloqueio recebem `0.0.0.0`
  (ou NXDOMAIN).
- **DNS criptografado**: as consultas liberadas saem por DNS-over-HTTPS (Quad9,
  Cloudflare), com UDP como reserva. Cache com TTL.
- **Listas de filtro**: HaGeZi Pro, HaGeZi Threat Intelligence, URLhaus,
  StevenBlack, telemetria Apple/Microsoft, apostas, adulto e redes sociais.
  Baixadas e atualizadas a cada 24 h. Aceita formato hosts, lista de domínios e
  `||dominio^`.
- **Bloqueio de CNAME cloaking**: cada CNAME da resposta também é checado.
- **Regras próprias**, na sintaxe do Portmaster: `- ads.exemplo.com`,
  `+ .apple.com`, `- *.tracker.net`, `- *metrics*`, `- 203.0.113.0/24`.
  A primeira que combinar decide, e regras valem antes das listas.
- **Bloqueio de IP** com o `pf` do macOS, num anchor próprio
  (`com.apple/portward`). O `/etc/pf.conf` não é editado.
- **Monitor de conexões por app**: lê os sockets de todos os processos via
  `libproc` (a mesma API do `lsof`), agrupa helpers no app principal e liga cada
  IP ao domínio que o originou.
- **Histórico** em SQLite local (7 dias por padrão).
- **App de desktop** (Tauri) com painel, atividade de rede, consultas DNS, listas,
  regras e configurações, mais um ícone na barra de menus para pausar e retomar.

## Arquitetura

```
crates/core     regras, listas de filtro, config e tipos da API (sem I/O, testável)
crates/daemon   portwardd: DNS, upstreams DoH/UDP, monitor libproc, pf, SQLite, API HTTP
ui/src-tauri    app Tauri (Rust) que fala com o daemon
ui/dist         interface (HTML/CSS/JS, sem bundler)
packaging/      plist do launchd
scripts/        build, instalação, remoção, gerador do ícone
```

O daemon roda como root via launchd. A interface roda como seu usuário e fala com
ele em `http://127.0.0.1:817`, autenticando com um token guardado em
`/Library/Application Support/Portward/api-token` (dono root, grupo admin, 0640).

## Instalar

Precisa de Rust (`brew install rustup && rustup default stable`) e Node (para a
CLI do Tauri).

```bash
./scripts/build.sh
```

```bash
sudo ./scripts/install.sh
```

Depois abra `/Applications/Portward.app`.

Ao iniciar, o serviço anota o DNS atual de cada interface de rede e troca por
`127.0.0.1`. Ao parar, devolve o original. Para remover tudo:

```bash
sudo ./scripts/uninstall.sh
```

Se a internet parar de resolver nomes e o serviço não estiver rodando, isto
devolve o DNS original:

```bash
sudo /usr/local/libexec/portward/portwardd restore-dns
```

## Testar sem root

```bash
cargo run -p portward-daemon -- --data-dir /tmp/pw run --dns-listen 127.0.0.1:5300 --api-listen 127.0.0.1:8817
```

```bash
dig @127.0.0.1 -p 5300 doubleclick.net
```

```bash
PORTWARD_API=http://127.0.0.1:8817 PORTWARD_TOKEN_FILE=/tmp/pw/api-token cargo run -p portward-ui
```

Sem root, o daemon só vê os seus próprios processos e não aplica regras de IP.

## Limitações (sem conta de desenvolvedor Apple)

O Portmaster original intercepta pacotes no kernel (Windows) ou via nfqueue
(Linux). No macOS isso exige um *Network Extension* assinado, com entitlements
da Apple e uma conta paga de desenvolvedor. Esta versão evita isso, e o custo é:

- **O bloqueio é global, não por app.** O Portward vê qual app abre cada
  conexão, mas o bloqueio de domínio vale para o sistema todo. Quase todo app
  resolve nomes pelo `mDNSResponder`, então o daemon não sabe qual app pediu cada
  consulta.
- **Apps com DNS próprio passam direto.** É o caso de navegadores com
  DNS-over-HTTPS ligado e de VPNs. Desligue o "DNS seguro" do navegador para que
  ele use o do sistema.
- **O monitor é por amostragem** (a cada 1 s). Conexões muito curtas podem não
  aparecer.
- **Não há SPN** (a rede de privacidade paga da Safing).

O próximo passo natural é um Network Extension: um shim pequeno em Swift
(`NEFilterDataProvider` + `NEDNSProxyProvider`) que repassa cada fluxo ao núcleo
em Rust, com bloqueio por app de verdade.

## Antivírus com inspeção de HTTPS

Antivírus que interceptam TLS (como o Kaspersky Web Anti-Virus) reemitem os
certificados dos servidores DoH. O Portward usa o repositório de certificados do
macOS, então funciona com eles. Alguns desses produtos também mexem no DNS e
podem disputar a porta 53. Se o serviço não subir, veja
`/Library/Logs/Portward/portwardd.log`.
