# Releases

## Contrato público

Uma tag `vX.Y.Z` deve coincidir com a versão de `[workspace.package]` em
`Cargo.toml`. Atualize o lockfile quando a versão mudar. O workflow
`.github/workflows/release.yml` roda ao enviar essa tag ao GitHub.

A release contém:

- `open-desktop-linux-x86_64.tar.gz`: binário, licenças, `BUILD.txt` com versão e
  commit, e `DEPENDENCIES.txt` com dependências dinâmicas;
- `SHA256SUMS`: checksums do pacote e do bootstrap;
- `install.sh`: bootstrap POSIX, compatível com `curl | sh`;
- nas versões com Mac: `open-desktop-macos-arm64.zip` e `SHA256SUMS-macos`,
  compilados no Mac a partir do mesmo commit, com assinatura de desenvolvimento.

O nome do pacote é estável; a URL da release fixa a versão. Isso permite instalar
`latest` sem interpretar JSON da API do GitHub. O instalador usa HTTPS, verifica
somente a entrada esperada do manifesto e não executa o binário quando o download,
o checksum ou as dependências falham. Checksums não são assinaturas independentes.

O bootstrap chama `opendesk install` e abre `setup` usando `/dev/tty`, inclusive
quando executado por `curl | sh`. Sem terminal, informa como configurar depois.
`OPENDESK_NO_SETUP=1` suprime o assistente; `opendesk update` usa essa opção. A CLI embute `scripts/install-user.sh` e a
unidade systemd: instalação local, download, atualização e remoção compartilham a
mesma rotina. A instalação exige sessão Hyprland Lua/UWSM ativa, preserva os
módulos existentes e recusa o arquivo principal Lua quando ele é um link simbólico.
Não escreve identificadores temporários da sessão.

## Preparar

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/check-file-length.sh
bash test/install-user-sandbox.sh
python3 test/bootstrap.py
cargo build --bin opendesk --locked
python3 test/setup-cli.py
cargo build --release --locked
bash scripts/package-release.sh v0.1.0
```

Use um checkout limpo para que `BUILD.txt` identifique exatamente o código do
artefato. O workflow usa Ubuntu 24.04 e Rust 1.98.0; a base glibc é anterior à dos
hosts Arch/CachyOS testados. Dependências são resolvidas pelo `Cargo.lock`.
O ambiente do runner e os pacotes do sistema podem receber atualizações: isto
não é uma promessa de builds idênticos bit a bit.

## Validar nas máquinas

Antes de anunciar uma release:

1. Instale **o mesmo pacote produzido pelo workflow** nos dois hosts e compare
   SHA-256, `opendesk --version` e dependências dinâmicas.
2. Rode `setup`: um host inicia, outro recebe e mostra o PIN. Confirme `all` em ambos.
3. Confira `status`, `doctor`, passagem pelas quatro bordas, retorno exclusivo,
   teclado, Ctrl+Alt+Esc, clipboard de texto/imagem e arrasto nos dois sentidos.
4. Atualize com o serviço ativo. Reinicie e confirme reconexão sem novo PIN.
5. Desinstale e reinstale; confirme que os dados e o pareamento persistem.

Os testes de bootstrap usam rede e distribuição simuladas. O teste sandbox usa
serviços simulados. Ambos verificam falhas e invariantes do instalador, mas não
provam compatibilidade do binário publicado com a sessão gráfica real.

## Publicar

Após criar o repositório público e configurar `origin`, envie o commit revisado e
uma tag anotada. Exemplo, executado pelo mantenedor:

```sh
git tag -a v0.1.0 -m 'Open Desktop v0.1.0'
git push origin HEAD
git push origin v0.1.0
```

O GitHub Actions executa os checks, constrói, empacota e cria a release com notas
automáticas em um rascunho. Complete o rascunho com o pacote Mac, verifique os
checksums e as notas antes de publicar. Tags contendo `-` geram prereleases. Não mova tags publicadas: corrija
com uma nova versão. O workflow não executa os testes gráficos aninhados.

Para voltar a um artefato publicado anterior: `opendesk update vX.Y.Z` nos dois
hosts. A primeira instalação registra backups dos arquivos pré-existentes;
atualizações mantêm esse registro para desinstalação. Os backups de cada tentativa
ficam em `~/.local/share/opendesk/backups` para recuperação manual.
