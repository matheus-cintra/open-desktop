# Mapa compartilhado — candidata instalada, aceite físico pendente

Atualizado em 2026-09-13. A versão **0.3.0-alpha.1, protocolo 4**, foi implementada,
compilada e instalada nos três computadores. O usuário autorizou a publicação pública como alpha experimental com pendências
documentadas em `release-0.3.0-alpha.1.md`. O aceite físico completo ainda não está concluído.

## Instalação verificada

| Máquina | Identidade | Versão | Estado após atualização |
| --- | --- | --- | --- |
| Desktop cachyos | e14e9fdb02d4477fa11769f66af29251 | 0.3.0-alpha.1 | input ready, dois peers conectados |
| Notebook archlinux | 5723c4608d704762ba32e1c0afd2cfcb | 0.3.0-alpha.1 | sessão bloqueada, dois peers conectados |
| MacBook | bdcb4aa1ab944406a7a3c214184d9368 | 0.3.0-alpha.1 | input ready, dois peers conectados |

Mapa inicial aplicado pelo IPC: **Notebook Linux — Desktop — MacBook**.
Dimensões lógicas obtidas das máquinas: 1920×1200, 3840×2160 e 2056×1329.
Revisão 1, autor desktop. O arquivo map.json tem SHA-256 idêntico nas três máquinas:
`e21dacb43393d7c101a71903c36408f7a8467678d57c2c3b31a0b258a0593db8`.

Binários instalados:

- Linux (mesmo arquivo nos dois computadores):
  `eaef471d0b40135f3620fe172a63eebc685a81e4234217c921f1c1bee8725cb8`.
- Mac:
  `49bf431ac887545da26e5582965f441fcbd4f917199a1273f45da836f18da73a`.

A exigência de assinatura do Mac antes/depois é idêntica: identificador
`dev.mcintra.opendesk`, certificado leaf
`0f939256b2da742cb81ad8f035a0e668c6526281`. Codesign verificou o bundle e a aplicação
instalada retornou input ready; não foi executado reset de TCC.

O auxiliar Linux está ativo nas duas máquinas, com usuário/grupo
`opendesk-activity` e grupo suplementar `input` somente no serviço. O usuário
comum não foi adicionado ao grupo input. A conexão local do daemon com o socket
do auxiliar foi observada no desktop. As janelas GUI foram abertas nas três máquinas.

## Validação automatizada

Logs em `docs/verification/map-20260913/` (diretório de logs ignorado pelo Git).

- Linux: **183 testes Rust passaram**, 1 teste de notificações ignorado.
- Mac: **117 testes Rust passaram**, usando os pacotes compatíveis com macOS.
  `cargo test --workspace` também tenta compilar o backend exclusivo Wayland;
  o comando correto no Mac seleciona opendesk/core/proto/platform/macos.
- Clippy Linux com `--workspace --all-targets -- -D warnings`: passou.
- Testes nativos Objective-C: teclado, geometria lógica e quatro bordas passaram.
  Esses testes não capturam/injetam entrada e não substituem uso físico.
- Auxiliar Python/evdev: **7 testes passaram**, incluindo sessões por seat,
  clientes por UID, dispositivos virtuais e reinício de contato multitouch.
- Instalador Linux em sandbox: idempotência, backups, rollback e falha transacional passaram.
- Sintaxe dos scripts e `git diff --check`: passaram.
- GUI Linux abriu com wgpu/Vulkan na GPU real. Foi corrigido um defeito no arraste
  acumulado, coberto por teste egui com frames de movimento e pausa.

Os testes do daemon cobrem três origens com destino direto, retorno sem liberar
captura entre destinos, gerações antigas, teclas/modificadores, timeout, recusa,
reivindicações simultâneas, contador persistido e reconciliado após restart,
revalidação no Commit, arraste bilateral sem terceira máquina, conflitos de mapa,
persistência e rejeição explícita de protocolo 3.

## Implementação

- Mapa por PeerId, grupo e revisão lógica; geometria sem sobreposição/canto e
  resolução por raio para pular destinos indisponíveis sem salto diagonal.
- Persistência atômica de mapa e contador de controle, IPC para mapa/identificação,
  edição otimista, versões conflitantes preservadas e sincronização antes de controle.
- Origem envia diretamente ao destino, com Prepare/Release/Commit, gerações de
  entrada, renovação a cada 250 ms e prazo de 1 segundo.
- Reivindicação por atividade física, liberação emergencial, preservação de
  modificadores e integração do arraste Linux↔Linux bilateral.
- GUI em processo separado, aplicação/cancelamento, identificação, descoberta/PIN,
  zoom, encaixe, estado de conexão/permissão e uma janela por máquina.
- Serviço evdev por conta dedicada; dispositivos associados ao seat e sessões
  locais ativas/desbloqueadas; notificações sem códigos de teclas ou texto.
- Menu Mac e launcher Linux; instalação conjunta usou OPENDESK_INSTALL_ACTIVITY=1.

## Proveniência e backups

- Worktree: `/home/matheus/dev/pessoal/open-desktop`.
- Branch: `feat/macos-apple-silicon`; HEAD: `4f83a8d`, com alterações não commitadas.
- A árvore já continha mudanças de suporte ao Mac; elas foram preservadas.
- Backup do diff inicial e arquivos não rastreados:
  `/home/matheus/.local/state/opendesk-work/pre-map-20260913/`.
- Backup instalado no desktop:
  `/home/matheus/.local/share/opendesk/backups/install.MkFac4`.
- Backup instalado no notebook:
  `/home/matheus/.local/share/opendesk/backups/install.Mxgc2Q`.
- Backup instalado no Mac:
  `/Users/matheuscintra/Library/Application Support/Open Desktop/backups/update.TspOyj`.
- Build Linux: `/tmp/opendesk-map-build`; build Mac isolado:
  `/tmp/opendesk-map-check-20260913`.

A execução anterior parou por disco cheio. A retomada usou builds sem símbolos de
DEBUG e sem incremental em /tmp. Nenhuma identidade ou confiança foi regenerada.

## Aceite físico

Confirmado pelo usuário em 2026-09-13, usando o mouse do desktop:
Desktop → Mac → Desktop → Notebook Linux → Desktop. As saídas e os retornos
pelas bordas correspondentes funcionaram perfeitamente, segundo o relato.
Esse percurso valida os dois vizinhos com origem no desktop.

Também confirmado pelo usuário em 2026-09-13, usando somente o mouse/trackpad do
notebook: Notebook Linux → Desktop → Mac → Desktop → Notebook Linux.
Esse percurso valida a passagem pelo intermediário e o retorno com origem no notebook.

O teste com origem no Mac revelou falha intermitente: ao passar do desktop para
notebook, o controle retornava ao Mac. Logs do notebook registraram quatro avisos
`no input from the controlling peer, releasing` às 20:11:50, 20:11:55, 20:12:09
 e 20:12:58, sem reinício dos processos. O watchdog legado de UDP usava o horário
anterior do link e podia liberar o destino entre Commit e o primeiro datagrama.
Correção: sessões de mapa usam somente a autorização renovável por geração,
com expiração de 1 segundo. Teste de regressão reproduziu a falha sem a correção
e passou com ela; também confirmou liberação por perda real da renovação.
Suíte do pacote opendesk: 49 testes passaram, 1 ignorado. Builds Linux/Mac passaram.
Atualização corretiva preserva versão/protocolo e assinatura do Mac.

Após a atualização, o usuário respondeu "aparentemente tudo certo" à repetição
do percurso Mac → Desktop → Notebook Linux → Desktop → Mac usando o trackpad do
Mac. Resultado inicial favorável; estabilidade prolongada ainda não comprovada.

Confirmado pelo usuário: com o Mac controlando o notebook, a atividade física
no mouse/trackpad do notebook assumiu o controle local e permitiu atravessar
para o desktop usando o notebook como nova origem.

Confirmado pelo usuário: com o notebook controlando o Mac, a atividade física
no trackpad do Mac assumiu o controle local e permitiu atravessar para o desktop
usando o Mac como nova origem.

Confirmado pelo usuário: com o Mac controlando o desktop, a atividade física
no mouse do desktop assumiu o controle local e permitiu atravessar para o
notebook usando o desktop como nova origem. Tomada física confirmada nas três
máquinas nos cenários acima.

Confirmado pelo usuário: após fechar as janelas de organização nas três máquinas,
o percurso Notebook → Desktop → Mac → Desktop → Notebook continuou funcionando.
Fechar a GUI sem interromper o compartilhamento está validado fisicamente.

Clipboard confirmado pelo usuário como funcional, com evidência enviada na
conversa: texto copiado do notebook e duas imagens, uma captura do notebook e
outra do macOS. O relato não discrimina todos os pares origem/destino; não
considerar essa evidência uma matriz completa de clipboard entre as três máquinas.

Confirmado pelo usuário: arraste de um arquivo de teste entre notebook Linux e
desktop funcionou nos dois sentidos, e o arquivo recebido abriu corretamente.

Confirmado pelo usuário: alteração pela GUI do Mac apareceu nas duas GUIs Linux;
restauração da disposição original pela GUI do notebook apareceu nas outras
máquinas. Edição e sincronização física confirmadas com Mac e notebook como autores.

O usuário reorganizou o mapa para **MacBook — Notebook — Desktop**. Nessa
disposição, confirmou Desktop → Mac e Mac → Desktop diretamente, pulando o
notebook com a sessão bloqueada. Salto do intermediário bloqueado confirmado
nos dois sentidos; não confundir com desconexão/offline.

Próximo teste: desconectar o notebook da rede e repetir Desktop → Mac e
Mac → Desktop, depois reconectar o notebook e verificar sua reintegração.

Pendentes: estabilidade prolongada com origem no Mac; extremo a extremo com
intermediário offline; edição pela GUI do desktop (Mac e notebook confirmados);
cursor local parado e retorno por bordas diferentes; Control Mac →
Super Linux; matriz completa de clipboard texto/imagem;
autorização/reinício automático no Mac. Medir transferência após o limiar (objetivo
≤250 ms) e recuperação por perda de comunicação (objetivo ≤1 s).

Não considerar latência, retomada física ou percurso nas máquinas reais provados
pelos testes automatizados. A funcionalidade está instalada para esse aceite.

## Verificação para publicação da alpha

A revisão para release repetiu os 183 testes Rust Linux, 117 testes Rust Mac,
Clippy, testes nativos Mac, instalador sandbox, 6 testes de bootstrap, 5 testes
do assistente CLI e 7 testes do auxiliar de atividade.
A suíte gráfica de quatro bordas passou 88/88 após corrigir o injetor do teste
para manter o dispositivo virtual entre movimentos; a versão estável também
falhava com o injetor recriado a cada comando. Essa suíte cobre o modo legado
sem mapa e não substitui o aceite físico do mapa relatado acima.
A suíte de bloqueio aninhada passou 66/66; também cobre o modo legado.
