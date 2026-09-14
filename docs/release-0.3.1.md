# Open Desktop 0.3.1

Atualização do macOS pela CLI, com backup e restauração automática:

- `opendesk update`, `opendesk update latest` ou uma versão explícita.
- Validação do download, checksum, bundle ARM64, versão e identidade de assinatura antes de fechar o app.
- Backup privado do app e dos dados; preservação de identidade, pares, mapa e preferências, sem reset de permissões.
- Reabertura da janela de organização e confirmação do processo e IPC.
- Restauração em caso de falha e recuperação de atualizações interrompidas.

## Instalação

Linux: `opendesk update v0.3.1`.
Mac com o updater instalado: `opendesk update v0.3.1`.
Mac com a v0.3.0 pública: instale o ZIP ARM64 conforme [as instruções](macos.md);
a partir da 0.3.1, as próximas atualizações podem usar a CLI.

O pacote Mac mantém a assinatura de desenvolvimento existente e não é notarizado.
O instalador recusa certificados diferentes. Protocolo 4 e formatos de dados
permanecem iguais; nenhuma alteração funcional do controle no Linux.

## Validação

Testes do updater em Linux e macOS, testes nativos de assinatura e restauração,
testes Rust, formatação e Clippy. No Mac real, atualização pública pela CLI com
janela aberta, preservação dos dados e assinatura, reabertura dos processos e
resposta IPC foram observadas; entrada final `ready` e dois pares conectados.
Veja o [relatório do updater](verification/macos-update-20260913/report.md).

A confirmação física específica de GUI, permissões e controle após esta atualização
continua pendente. Permanecem os [limites da versão 0.3.0](release-0.3.0.md),
incluindo suporte experimental ao macOS, uma tela ativa por máquina e ausência de
notarização. Testes automatizados não substituem a validação física.
