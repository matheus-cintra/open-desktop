# Open Desktop 0.3.2

Corrige o setup ao configurar um computador já pareado quando há múltiplos peers.

- Preserva a borda existente, evitando o conflito ao tentar usar `all`.
- Exibe a posição de cada computador na lista.
- Para peers sem posição, oferece bordas livres quando há outras posições ocupadas.
- Recusa nomes desconhecidos e bordas ocupadas sem alterar posições existentes.

Atualize com `opendesk update v0.3.2`.

A correção tem cobertura de regressão em dez cenários da CLI com terminal e IPC
isolados. Permanecem os limites de suporte e validação física da
[versão 0.3.1](release-0.3.1.md).
