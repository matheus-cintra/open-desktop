<div align="center">
  <img src="docs/icon.svg" width="96" alt="Open Desktop" />
  <h1>Open Desktop</h1>
  <p><strong>Seu mouse atravessa a tela. Seu trabalho continua.</strong></p>
  <p>Compartilhe mouse, teclado, clipboard e arquivos entre dois computadores com Hyprland.</p>
  <p>
    <img src="https://img.shields.io/badge/plataforma-Arch%20%2F%20CachyOS-74e1d0?style=flat-square" alt="Arch e CachyOS" />
    <img src="https://img.shields.io/badge/Hyprland-Lua-b2a1ff?style=flat-square" alt="Hyprland Lua" />
    <img src="https://img.shields.io/badge/licença-MIT%20%2F%20Apache--2.0-blue?style=flat-square" alt="MIT ou Apache 2.0" />
  </p>
  <p><a href="#instalação">Instalar</a> · <a href="#primeira-conexão">Conectar</a> · <a href="#comandos">Comandos</a> · <a href="#compatibilidade">Compatibilidade</a></p>
  <img src="docs/overview.svg" width="800" alt="Ilustração: desktop e notebook compartilham controles pela rede local" />
</div>

<table>
<tr><td width="50%"><h3>↗ Quatro bordas</h3>Empurre o cursor contra qualquer borda. Uma barra mostra o progresso até a passagem.</td><td><h3>⌨ Teclado acompanha</h3>Digite e use atalhos no computador que está recebendo o cursor.</td></tr>
<tr><td><h3>▣ Copie aqui, cole lá</h3>Textos e imagens, incluindo capturas de tela, sincronizam entre as máquinas.</td><td><h3>⇄ Arraste arquivos</h3>Leve o arquivo até a outra tela e solte no gerenciador de arquivos.</td></tr>
</table>

## Instalação

[Baixar a release mais recente](https://github.com/matheus-cintra/open-desktop/releases/latest) · [Instalar pelo código](#pelo-código)

Nos **dois computadores**, abra um terminal na sessão Hyprland iniciada com UWSM:

```sh
curl -fsSL https://github.com/matheus-cintra/open-desktop/releases/latest/download/install.sh | sh
```

Sem `sudo` e sem compilar Rust. O instalador verifica o SHA-256 do pacote, instala em `~/.local/bin`, habilita o início automático e configura a integração do Hyprland. Ao terminar, o instalador abre automaticamente o assistente, que inicia o serviço e orienta o pareamento. Sem terminal interativo, execute `opendesk setup` depois. Adicione `~/.local/bin` ao `PATH` para usar apenas `opendesk`.

Prefere inspecionar o script? Baixe o `install.sh` da release, leia e execute `sh install.sh`. Para uma versão específica: `sh install.sh v0.1.0`.

## Primeira conexão

1. Instale nas duas máquinas, conectadas à mesma LAN. O assistente abre automaticamente; para voltar a ele depois, execute `opendesk setup`.
2. Em uma, escolha **Iniciar pareamento** e selecione o outro computador.
3. Na outra, escolha **Receber**. Digite o PIN exibido nela no computador que iniciou.
4. O assistente configura as quatro bordas em cada máquina. Se já estiverem pareadas, escolha **Configurar peer existente**.
5. Empurre o cursor contra uma borda até a barra completar. O teclado acompanha.

Você entra pela borda oposta, na mesma posição proporcional. Para voltar, use **somente a borda de entrada**; as outras três não devolvem o controle. **Ctrl+Alt+Esc** libera o controle imediatamente.

O app funciona em segundo plano. Nenhum terminal precisa permanecer aberto. Clipboard sincroniza mesmo quando o cursor está local.

## Comandos

| Quero… | Comando |
|---|---|
| Configurar ou parear | `opendesk setup` |
| Ver conexão, endereço e bordas | `opendesk status` |
| Encontrar computadores | `opendesk discover` |
| Pausar / retomar passagens | `opendesk pause` / `opendesk resume` |
| Recuperar o controle local | `opendesk release` |
| Iniciar / parar / reiniciar | `opendesk start` / `stop` / `restart` |
| Diagnosticar | `opendesk doctor` |
| Acompanhar logs | `opendesk logs` — Ctrl+C fecha os logs |
| Atualizar | `opendesk update` |
| Instalar uma versão anterior | `opendesk update v0.1.0` |
| Desinstalar, preservando dados | `opendesk uninstall` |

Pausar impede novas passagens; não desliga a sincronização do clipboard. Use `stop` para desligar o serviço inteiro. `enable` e `disable` continuam disponíveis como aliases de `resume` e `pause`.

<details>
<summary>Pareamento e bordas manualmente</summary>

```sh
opendesk pair NOME
# Leia o PIN usando opendesk status no outro computador.
opendesk peer set NOME --side all
opendesk peer set NOME --side left
opendesk peer remove NOME
```

Também são aceitos `right`, `top`, `bottom` e a sintaxe anterior `peer set NOME left`. Um peer com `all` não pode disputar bordas com outro peer. Configure a posição em cada computador.
</details>

## Compatibilidade

| Suporte inicial | Escopo |
|---|---|
| Sistema | Arch Linux e CachyOS, x86_64 |
| Sessão testada | Hyprland **0.56.2**, configuração **Lua**, gerenciada pelo **UWSM** |
| Telas | Dois computadores, um monitor em cada; resoluções diferentes aceitas |
| Rede | Mesma LAN confiável; TCP/UDP **47820**, mDNS UDP **5353** |
| Clipboard | Texto e imagens |
| Arquivos | Arrasto entre aplicações compatíveis, validado com Thunar |

**O transporte ainda não é criptografado. Use somente em rede local confiável.** O PIN pareia os dispositivos; não fornece criptografia do tráfego. Execute a mesma versão nas duas máquinas. O protocolo atual é 2; versões de protocolo incompatíveis são rejeitadas.

Não há suporte anunciado para outros compositores, configuração Hyprland antiga em `.conf`, múltiplos monitores por máquina, macOS ou Windows. O instalador não altera o firewall automaticamente. Se a descoberta falhar, confira as portas acima e o isolamento de clientes do Wi-Fi.

## Atualização e remoção

A atualização substitui o binário de forma atômica e reinicia o serviço se ele estava ativo. Falhas na instalação restauram os arquivos substituídos e o estado anterior do serviço. Configuração, identidade e pareamento são preservados.

```sh
opendesk update
opendesk uninstall
```

Backups: `~/.local/share/opendesk/backups`. Dados preservados: `~/.config/opendesk` (ou os diretórios XDG configurados). Nunca copie identidades entre computadores. Depois de reinstalar, `setup` permite selecionar o peer existente.

## Pelo código

Requer Rust **1.98**, compilador C, `pkgconf`, Wayland e libxkbcommon. Com as dependências instaladas:

```sh
cargo build --release --locked
./target/release/opendesk install
~/.local/bin/opendesk setup
```

## Desenvolvimento

[Arquitetura e testes](docs/technical.md) · [Processo de release](docs/releases.md)

Mouse, quatro bordas, teclado, clipboard de texto/imagens, arrasto e liberação de emergência foram validados fisicamente no par CachyOS/Arch. Cada nova release também exige verificação dos artefatos; testes de CI não substituem a validação nas sessões reais.

Distribuído sob **MIT ou Apache-2.0**, à sua escolha.
