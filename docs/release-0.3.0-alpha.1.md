# Open Desktop 0.3.0-alpha.1

- Janela para organizar e sincronizar a disposição dos computadores.
- Controle contínuo entre três máquinas, com teclado/mouse de qualquer uma.
- Atividade física em outra máquina assume o controle.
- Suporte experimental a Apple Silicon/macOS 26, além de Linux Hyprland.
- Correção de uma liberação intermitente durante a troca de destino.

## Instalação

Use esta mesma versão em todos os computadores: protocolo **4**, incompatível
com versões anteriores. Identidades e pareamentos são preservados.
No Linux, execute `opendesk update v0.3.0-alpha.1` ou o instalador desta tag.
Instale o auxiliar de atividade com `opendesk install-activity` (sudo) e abra
`opendesk gui`. No Mac, use o ZIP ARM64 e as [instruções de instalação](macos.md).
O pacote Mac tem assinatura de desenvolvimento; não é notarizado.

## Validação e limites

Confirmados fisicamente em três máquinas: percursos e retornos; tomada de controle
em cada máquina; salto do intermediário bloqueado nos dois sentidos; edição e
sincronização pelo Mac e notebook; clipboard de texto e imagens nos casos testados;
arraste Linux ↔ Linux nos dois sentidos; continuidade após fechar as janelas.
A atualização do Mac preservou a assinatura e as permissões existentes.

Ainda não concluídos: intermediário offline e reconexão; edição pela GUI do desktop;
verificação específica do cursor da origem e retorno por outra borda; Control do
Mac → Super do Linux; matriz completa de clipboard; nova autorização com reinício
automático; medição física de transferência ≤250 ms e recuperação ≤1 segundo;
estabilidade prolongada. O usuário autorizou a publicação experimental com essas
pendências. Testes automatizados não substituem esses resultados físicos.

Uma tela ativa por computador. Finder DnD e arraste atravessando uma terceira
máquina não são suportados. Use somente em LAN confiável: o transporte ainda não
é criptografado. A release está marcada como pré-release e não substitui `latest`.

## Manutenção

O verificador de tamanho mantém limites explícitos para sete módulos ampliados
nesta integração alpha; os demais continuam limitados a 300 linhas. Esses limites
impedem crescimento adicional até a extração dos módulos de GUI e estado do mapa.
