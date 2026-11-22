# AGENTS.md — guia para agentes de IA

**speedconquest**: conquistas offline estilo PS5 para o **Need for Speed Underground 2** (PC,
`SPEED2.EXE` 32 bits) rodando no **Wine, no Linux**. Um processo à parte lê a memória do jogo pelo
`/proc/<pid>/mem` (sem injetar nada), avalia as conquistas, grava num SQLite e mostra um popup por cima
do jogo no gamescope. Também tem uma janela egui com a lista. Rust, edição 2024.

**Escopo:** o projeto é **exclusivo do NFSU2**. Não proponha suporte a outros jogos nem mencione
outros jogos no código ou na documentação.

## Leia primeiro
- `README.md`: o que faz, estado, requisitos, uso, lista de conquistas.
- `jogos/nfsu2/NOTAS.md`: endereços de memória, estruturas do jogo, como cada coisa foi achada e as
  receitas de teste de ponta a ponta.
- `PLANO.md`: diário de desenvolvimento (decisões fechadas, roteiro e estado de cada versão).
  Ao fechar uma versão, acrescente uma entrada na seção *Estado*.

## Comandos
```bash
cargo build --release          # binário em target/release/speedconquest
cargo nextest run              # ou cargo test (sem doctests: o crate é só binário)
cargo clippy --all-targets     # tem de sair sem warnings
cargo fmt
python3 ferramentas/conquistas_md.py jogos/nfsu2/conquistas.toml   # tabela do README
```
Rust 1.95 ou mais novo. Precisa de um compilador C (o SQLite é compilado pelo `rusqlite`).

## Mapa do código
| Arquivo | Papel |
|---|---|
| `src/main.rs` | subcomandos, vigia a 30 Hz, fila de popups, platina |
| `src/processo.rs` | acha o processo, base do módulo (`/proc/pid/maps`), sha256 do exe, leitura e cadeias de ponteiros |
| `src/config.rs` | `jogo.toml` + `conquistas.toml` + `listas.toml`, embutidos com `include_str!` e validados |
| `src/avaliador.rs` | transições, marcos 25/50/75%, derivadas (odômetro, velocidade, saltos, giro, corridas…) |
| `src/banco.rs` | SQLite: `desbloqueio`, `marco`, `estatistica`, `maior`, `historico` |
| `src/popup.rs` | popup (tiny-skia + ab_glyph), janela X11 marcada `GAMESCOPE_EXTERNAL_OVERLAY` |
| `src/janela.rs` | janela egui (eframe 0.37, glow + x11) |
| `jogos/nfsu2/` | definição do jogo, conquistas e listas de hashes das corridas |

## Regras do projeto
- **Disparo por evento, nunca por estado**: uma conquista sai quando o valor *cruza* a meta durante
  o jogo, não porque o save já tem aquilo. Ao carregar um perfil, as primeiras leituras são só base.
- **Progresso por perfil**: a chave é (jogo, nome do perfil, conquista). Outro perfil começa do zero.
- **Lista de conquistas**: mudanças (nome, meta, nível, conquista nova) precisam de aprovação do
  mantenedor. Ao mudar a lista, atualize a tabela do README (script acima) e a contagem no teste
  `config::testes::embutidos_carregam`.
- **Endereços novos**: documente no `NOTAS.md` o endereço, o tipo e como foi conferido. Exe diferente
  do sha256 aceito no `jogo.toml` é recusado de propósito.
- **Nunca** use o banco real (`~/.local/share/speedconquest/conquistas.db`) em testes: use
  `SPEEDCONQUEST_DB=<arquivo de teste>`. Ele guarda o progresso de verdade de quem joga.
- **Nunca** faça commit de código descompilado do jogo nem de arquivos do jogo (`ghidra/*.txt` está
  no `.gitignore`). Endereços e nomes de função nas notas podem.
- Sem mecanismo anti-trapaça (é offline e de um jogador só).
- Código, comentários, mensagens e documentação em **português do Brasil**.
- Erros com `anyhow` e `Result`. Nada de `unwrap()`/`expect()` fora dos testes sem um comentário
  explicando a invariante. Casts numéricos com `#[allow(clippy::…)]` comentado.
- Bug corrigido ganha teste de regressão.

## Testes de ponta a ponta
Os testes de unidade não substituem rodar o jogo. A receita, detalhada no `NOTAS.md`: abrir o jogo
num gamescope sem tela (`--backend headless`), carregar um **perfil de teste**, rodar o vigia com
`SPEEDCONQUEST_DB` apontando para um banco de teste e escrever valores na memória do jogo
(`/proc/<pid>/mem`, de preferência com o jogo pausado, porque a física reescreve posição e
velocidade todo quadro). `SPEEDCONQUEST_DEBUG=1` mostra os valores lidos uma vez por segundo. Para
ver o popup no print, peça ao gamescope uma captura da composição completa
(`GAMESCOPECTRL_REQUEST_SCREENSHOT` = 3 no root window do display dele).

## Limitações conhecidas
Só Linux e X11 (ver "E no Windows?" no README). O replay soma no odômetro, nas cambalhotas e nos
saltos. Sem som no popup.
