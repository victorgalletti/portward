# Aviso

Portward é uma reimplementação independente em Rust, para macOS, inspirada no
[Portmaster](https://github.com/safing/portmaster) da Safing ICS Technologies GmbH
(GPLv3). Nenhum código Go do Portmaster foi copiado. A paleta de cores e o
formato das regras de endpoint (`+ dominio` / `- dominio`) seguem o original.

"Safing", "Portmaster" e seus logos são marcas registradas da Safing ICS
Technologies GmbH. Por isso este projeto usa outro nome e um ícone próprio, e não
tem nenhuma ligação com a Safing.

Licença: GPL-3.0-or-later (veja LICENSE).

## Componentes de terceiros

- MapLibre GL JS (BSD-3-Clause), em `ui/dist/vendor/maplibre`.
- Mapas: OpenFreeMap, © OpenMapTiles, dados © colaboradores do OpenStreetMap.
- Mapa offline de reserva: Natural Earth 1:50m (domínio público), no formato
  TopoJSON do projeto world-atlas (ISC, Mike Bostock), em `scripts/world-50m.json`.
- País e empresa de IPs: IP Geolocation by DB-IP (CC BY 4.0).
- Localização aproximada dos seus IPs públicos: ipinfo.io.

O logo e o ícone do Portward são originais: inspirados no estilo retrô
"inline" da série Mr. Robot, sem usar o logo nem a máscara da fsociety.
