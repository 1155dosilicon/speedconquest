# speedconquest — plano

Sistema de conquistas estilo RetroAchievements/PS5 para o Need for Speed Underground 2 (PC)
rodando no Wine, offline, sem conta. **Exclusivo do NFSU2.**
Código em Rust, aqui em `~/src/speedconquest`.

## Como trabalhar (regras do usuário)
- **Protótipo mínimo e ir acrescentando.** Nada de analisar o jogo inteiro antes.
- **Na dúvida, o que der MENOS trabalho / mais simples**, sem perguntar.
- **Contexto limpo:** a cada marco (fim de versão, descoberta grande) atualizar a seção
  *Estado* abaixo e os arquivos de `jogos/<jogo>/`, e sugerir ao usuário abrir uma sessão nova.
  Uma sessão nova começa lendo este arquivo.
- **Lista de conquistas:** eu proponho, o usuário aprova ou edita e aprova. Nada entra sem aprovação.
- **Testes:** no `tela-gpu` (fora da tela do usuário), com saves meus (pode criar quantos quiser).
  Tela principal (:0) só se for muito complexo/necessário, e **sempre avisando antes**.
- Sem mecanismo anti-cheat/varredura de trainer (trabalho desnecessário).

## Decisões fechadas
| Tema | Decisão |
|---|---|
| Leitura | Processo externo lê a memória do jogo (`process_vm_readv`). Sem injetar DLL. `ptrace_scope`=0 → não precisa root. No Wine o endereço Windows = endereço Linux. |
| Fontes de dados | Memória ao vivo (principal). Save só se for mais fácil para algo específico. |
| Endereços | Modelo `módulo + deslocamento + cadeia de ponteiros` desde o início; padrão de bytes (AOB) se um dia precisar. Versão do exe identificada por **sha256**; exe desconhecido → recusa. |
| Disparo | **Por evento/transição** ("acabou de vencer"), nunca por estado ("tem vencido"): progresso antigo de save não desbloqueia. |
| Progresso | **Um conjunto por save**: chave do banco = (jogo, nome do save, conquista). Load save X = conquistas do X; save Y = do Y. Tudo começa do zero. |
| Estatísticas próprias | Odômetro, giro, contadores: calculados por nós e guardados por save. Tipos declarados no `jogo.toml` e implementados em Rust (`avaliador.rs`); Lua só se um dia não der (v0.3: não precisou). |
| Níveis | Bronze / prata / ouro + **platina automática** (todas as outras). |
| Popup | Fundo **sólido** (gamescope mistura em luz linear: 91% de alfa parecia ~66%). Título em **Grenze Gotisch**, texto em **Titillium Web** (OFL, embutidas, `assets/fontes/`; escolha do usuário 2026-11-21); tamanhos normalizados pela altura do "x". Compacto: 60 px de altura, largura pelo texto (200–340 px a 768 linhas; usuário achou 440 fixo comprido demais na TV). Canto superior direito a **8 px** do topo e da borda (`MARGEM`, pedido do usuário). Barra fina estilo PS5: troféu (cor do nível), nome, descrição curta embaixo (resumo se longa). Ícone e som **próprios** (nada da Sony). Aparece no desbloqueio **e no progresso** (25/50/75%). |
| Popup sobre o jogo | Plano A: janela overlay do gamescope (`GAMESCOPE_EXTERNAL_OVERLAY`); jogos rodam via `jogo-gamescope`. Plano B: camada Vulkan (tudo passa por DXVK). |
| UI | Janela egui (lista, %, barras de progresso) + o popup. |
| Banco | SQLite local. |

## Arquitetura (alvo)
```
speedconquest/
├── núcleo: detector de jogo (nome do processo + sha256 do exe) → leitor de memória
│           (30–60 Hz) → avaliador (condições, valor anterior, "durante N quadros")
│           → banco SQLite → aviso (popup)
├── popup/      processo separado, overlay do gamescope
├── janela/     egui (v0.4)
└── jogos/
    └── nfsu2/  jogo.toml (exe aceitos + mapa de variáveis) · conquistas.toml · listas.toml · NOTAS.md
```
Exemplo de formato (rascunho, simplificar se der):
```toml
# jogo.toml
[exe]
nome = "speed2.exe"
sha256 = ["f9dd86c054878ce6276beb07c1fd61874f7a1e4bf1f241b084c65b73e24168a7"]
[var.dinheiro]   tipo = "u32"; endereco = "speed2.exe+0x?????"
[var.perfil]     tipo = "str16"; endereco = "[speed2.exe+0x?????]+0x10"

# conquistas.toml
[[conquista]]
id = "dinheiro_10k"; nivel = "bronze"
nome = "Primeiros US$ 10.000"; descricao = "Junte US$ 10.000 na carreira."
progresso = { var = "dinheiro", meta = 10000 }
```

## Roteiro
- **v0.1 (NFSU2):** detectar `speed2.exe`; ler nome do save + dinheiro; 3 conquistas de dinheiro
  (10k/100k/1M) com progresso; popup sobre o jogo no gamescope; gravar por save.
  → valida leitura externa + popup no gamescope.
- **v0.2:** velocidade, pontuação de drift → primeiras conquistas absurdas.
- **v0.3:** cambalhotas, tempo acima de 250 km/h, platina (feito). Salto adiado.
- **v0.3b:** carreira (feito) + resultado de corrida (feito: circuito 30 s, drag perfeito).
- **v0.4:** janela egui com a lista.
- Depois: a definir com o usuário.

## Rascunho de conquistas (NÃO aprovado ainda; números com ? só depois de medir)
[S]=save/estado fácil · [M]=memória ao vivo · [C]=calculado por nós

**NFSU2 principais:** 🥉 primeiro patrocínio [S] · 🥉 primeira capa de revista [S] · 🥈 capa de DVD [S] ·
🥈 concluir Stage 1…5 [S] · 🥇 vencer todos os URL [S] · 🥇 zerar a carreira [S] · 🏆 platina.
**NFSU2 absurdas:** 🥇 1M de drift num evento? [M] · 🥈 drag só com trocas perfeitas [M] · 🥈 >350 km/h? [M] ·
🥇 salto de 100 m? [C] · 🥉🥈🥇 odômetro 100/1.000/5.000 km [C] · 🥈 circuito com 30 s de vantagem [M] ·
🥈 Street X sem bater [M] · 🥉 50 outruns [C] · 🥈 vencer com carro de série [M].

## Ambiente (fatos conferidos em 2026-11-21)
- Prefixo `~/.wine` (win64+wow, WineHQ staging 11.22, DXVK). Mais detalhes: memória `wine-games`.
- NFSU2: `C:\Program Files (x86)\EA GAMES\Need for Speed Underground 2\SPEED2.EXE`
  sha256 `f9dd86c054878ce6276beb07c1fd61874f7a1e4bf1f241b084c65b73e24168a7`.
  Com WidescreenFix (dinput8 ASI) + Unlimiter + mod do Integra.
  Saves em `C:\users\eli\AppData\Local\NFS Underground 2\<PERFIL>\`. Perfis: `UNIX` (criado sozinho,
  carreira começada com o Integra, US$ 75) e `TESTEB` (meu, de teste, sem carreira). Ambos são de teste.
  Endereços conferidos: ver `jogos/nfsu2/jogo.toml`.
- Ferramentas: `scanmem` (CLI, sem root — a minha), `pince` (GUI, root), `ghidra`/`ghidra-headless`, `imhex`.
- Testes com GPU: `tela-gpu start --mudo -- wine ...`, `tela-gpu shot`, `tela-gpu key`.
  NFSU2: `cd <pasta do jogo> && tela-gpu start --mudo -- wine SPEED2.EXE`; Enter, Enter → menu principal
  (~20 s). Profile Manager = 5× Right no menu principal. Nome do perfil: BackSpace×4 + `tela-gpu type`.
- **Print com o popup:** `tela-gpu shot` só pega o jogo. Para ver overlays:
  `DISPLAY=$(tela-gpu display) xprop -root -f GAMESCOPECTRL_REQUEST_SCREENSHOT 32c -set GAMESCOPECTRL_REQUEST_SCREENSHOT 3`
  → `/tmp/gamescope.png` (tipo 3 = composição completa).
- **Teste de ponta a ponta:** `SPEEDCONQUEST_DB=<arquivo de teste> speedconquest` e mexer no valor
  gravando em `/proc/<pid>/mem` (python, `r+b`, seek no endereço). Nunca usar o banco real em teste.

## Estado
- 2026-11-21: só planejamento. Ferramentas instaladas.
- 2026-11-21: **v0.1 feita e testada no tela-gpu** (commit inicial). Falta só a aprovação da lista.
  - Código: `src/config.rs` (toml embutido via include_str!), `processo.rs` (acha o pid pelo `comm`,
    base do módulo pelo `/proc/pid/maps`, sha256 do exe, lê `/proc/pid/mem`, cadeia de ponteiros),
    `avaliador.rs` (transições + 30 leituras de base ao anexar/trocar perfil; marcos 25/50/75 — só o maior
    cruzado), `banco.rs` (SQLite, tabelas `desbloqueio` e `marco`), `popup.rs` (tiny-skia + ab_glyph,
    janela X11 ARGB com `GAMESCOPE_EXTERNAL_OVERLAY`), `main.rs` (vigia a 30 Hz, fila de popups:
    um processo `speedconquest popup …` por vez, com DISPLAY/XAUTHORITY lidos do `environ` do jogo).
  - Comandos: `speedconquest` (vigia), `speedconquest lista`, `speedconquest popup NIVEL TITULO LINHA2 [PCT]`,
    `speedconquest png saida.png`. Banco real: `~/.local/share/speedconquest/conquistas.db` (vazio).
    Binário release instalado em `~/.local/bin/speedconquest` (cópia, reinstalar após mudar).
  - Endereços NFSU2: perfil = `SPEED2.EXE+0x43A9E0` (texto 16; vale "No Profile" antes de carregar save → `sem_perfil`), dinheiro = `SPEED2.EXE+0x461E74` (i32).
    Nome e dinheiro trocam no mesmo instante (amostra de 20 ms) ao carregar outro perfil.
  - Descobertas: o gamescope **ignora a posição** da janela overlay (põe em 0,0) → no gamescope a janela
    cobre a tela toda, transparente, e o popup é desenhado no canto superior direito (detecção: o átomo
    `GAMESCOPE_EXTERNAL_OVERLAY` já existe no X). Fora do gamescope: janela override-redirect pequena.
  - Testado: marco 25% (US$ 3.000), desbloqueio (US$ 12.000), troca de perfil UNIX↔TESTEB sem disparo
    falso, conjunto separado por perfil, fila de 2 popups seguidos, jogo fechando → volta a esperar.
  - Testado também na tela real (:0) com `jogo-gamescope --ss 1 -- wine SPEED2.EXE` (backend SDL): os 6 popups apareceram (usuário viu).
  - **Não testado:** sem gamescope (Wine direto
    em tela cheia no :0, sem garantia de ficar por cima). NFSU2 ainda não tem lançador com gamescope. Sem som no popup ainda. Sem platina ainda (só 3 conquistas).
  - Visual revisto com o usuário (2026-11-21): fundo sólido + fontes Grenze Gotisch/Titillium Web.
    Para testar outra fonte: `SPEEDCONQUEST_FONTE_TITULO=arq.ttf SPEEDCONQUEST_FONTE_TEXTO=arq.ttf speedconquest png x.png`.
  - Calibração (2026-11-21): 1M de saldo é inviável (carreira inteira paga ~US$ 548 mil em corridas,
    ver `jogos/nfsu2/NOTAS.md`). Criadas a derivada `ganhos` (soma dos aumentos de dinheiro, por perfil,
    tabela `estatistica`) e o histórico de mudanças (`registrar = true`, tabela `historico`,
    `speedconquest relatorio`). Lista **APROVADA**: 🥉 saldo 10k · 🥈 ganhos 100k · 🥇 ganhos 500k. Usuário não quer monitoramento: histórico desligado no NFSU2 (calibrar metas por dados do jogo, não medindo ele jogar).
  - Lançador `~/.local/bin/nfsu2` (cópia em `extras/nfsu2`): grava g_RacingResolution, sobe o
    `speedconquest` se não estiver rodando (log `~/.cache/jogos/speedconquest.log`), abre no
    `jogo-gamescope --ss 1` e derruba o vigia ao fechar. Menu: `~/.local/share/applications/wine/nfsu2.desktop`
    (backup em ~/.local/share/backups). `--ss 2` (2732x1536) faz o SPEED2.EXE fechar com page fault
    (leitura em 0x38 a partir de 0x0073927D) logo ao abrir — não investigado; padrão é 1×.
  - Vigia inicia junto do lançador `nfsu2` (resolvido).
- 2026-11-21: **v0.2 parte 1 feita** (commits 650f62c + 6a1f6df fmt). Falta a lista ser aprovada.
  - Achados (detalhes e método em `jogos/nfsu2/NOTAS.md`): posição do carro `SPEED2.EXE+0x42BE80`
    (vec3, metros; velocidade escalar não existe pronta → calculada), pontuação de drift
    (total/melhor do evento) por cadeia a partir de `0x8900AC`, achada no Ghidra a partir do
    ExtraOptions. `UnlockAllThings` em `0x838464` destrava o Quick Race no jogo de teste.
  - Código: tipos `vec3`/`f32`; derivadas `distancia_de` (odômetro em m, ignora teletransporte
    > 540 km/h e tremidinha < 1 cm) e `velocidade_de` (km/h, média de 0,5 s, só ao vivo);
    `marcos = false` para recordes; estatísticas gravadas a cada 10 s / troca de perfil / fim.
    `SPEEDCONQUEST_DEBUG=1` imprime os valores 1×/s. `ferramentas/varre.py` = varredura de memória.
  - Testado ao vivo: velocidade do vigia ≈ velocímetro; odômetro somando; cadeia do drift nula
    fora de drift. **Não testado:** conquista de drift de ponta a ponta (fazer gravando o float),
    replay (pode somar km no odômetro).
- 2026-11-21: **v0.2 feita, testada e instalada.** Lista APROVADA e criada (`conquistas.toml`, 12 no total):
  velocidade 🥉200 🥈300 🥇350 km/h (sem marcos) · odômetro 🥉100 🥈1.000 🥇5.000 km (com marcos) ·
  drift 🥉100 mil/evento 🥈1 drift de 25 mil 🥇500 mil/evento (sem marcos).
  Teste de ponta a ponta de 7 das 9 novas no tela-gpu (odômetro 1.000/5.000 só nos testes de unidade, mesmo mecanismo) (método em `jogos/nfsu2/NOTAS.md`). `speedconquest png X.png`
  agora desenha o popup de todas as conquistas. Binário release + lançador reinstalados; banco real vazio.
  - Limitação aceita: assistir replay soma km no odômetro (flag de replay não achada; ver NOTAS).
  - Próximo (v0.3): salto (z da posição; metros de voo calculados por nós) e outras absurdas do rascunho
    (outruns, Street X sem bater, circuito com 30 s de vantagem…), platina. Propor lista antes.

- 2026-11-21: **v0.3 feita, testada e instalada** (18 conquistas). Lista APROVADA: 🥉 Capotou! (360°) ·
  🥈 Cambalhota dupla (720°) · 🥇 Circo de Bayview (1.080°) · 🥈 Dublê (25 voltas somadas, com marcos) ·
  🥈 Pé embaixo (30 s seguidos ≥ 250 km/h) · 🏆 Lenda do Underground (platina automática).
  - Saltos saíram nesta versão (100 m impossível, sem detector) e voltaram logo depois (abaixo).
  - Novas derivadas: `giro_de` (meias-voltas do vetor "pra cima" com histerese ±0,5; acaba após 1 s
    de pé; salto > 90° entre leituras = jogo reposicionando, zera), `voltas_de` (guardada),
    `tempo_acima_de` + `limite` (tolera quedas/falhas < 0,5 s). Platina = conquista `nivel = "platina"`
    sem `progresso`; o vigia desbloqueia quando todas as outras do perfil estão feitas.
  - Achado: objeto do carro `[[0x8900AC]+4]` com velocidade/orientação/rodas (NOTAS).
  - Limitação aceita: virar de teto e voltar pelo mesmo lado conta 360°; replay conta cambalhota/voltas.
  - **Banco real tem progresso do usuário** (perfil UNIX: velocidade_200 em 2026-11-21 20:00, 16 km):
    não mexer, testes só com `SPEEDCONQUEST_DB`.
- 2026-11-21: **saltos de volta** (22 conquistas). O usuário indicou um salto (morro do centro, captura
  em ~/Imagens/Capturas) e pulou 3× com um gravador ligado (com o ok dele; gravação apagada).
  Lista APROVADA: 🥉 Decolagem 30 m · 🥈 Voo rasante 50 m · 🥇 Sem asas 75 m (um salto, ao vivo) ·
  🥈 Milhas aéreas 1 km somado (com marcos). Detector e câmera lenta: ver NOTAS. Tipo novo de var
  `floats` (`quantos`, `passo`); derivadas `salto_de`/`saltos_somados_de` + `rodas` + `velocidade_vetor`.
  Testado no jogo (pausado, escrevendo no objeto do carro). Instalado.
  - Próximo: v0.3b (estado de corrida) ou v0.4 (janela egui). Perguntar ao usuário. → v0.3b.
- 2026-11-21: **v0.3b parte 1 (carreira) feita, testada e instalada** (40 conquistas). Lista APROVADA:
  prólogo 🥉, Stage 1 🥉, 2 🥉, 3 🥈, 4 🥈, 5 🥇 (Rei de Bayview) · Patrocinado 🥉 · Capa de revista 🥉 ·
  Capa de DVD 🥈 · Garoto-propaganda 🥇 (30 capas) · Liga URL 🥈 (24) · Fujão 🥈 (50 outruns) ·
  Especialista 🥈 ×5 (circuito 56, sprint 36, Street X 36, drag 34, drift 36) · Invicto 🥇 (222).
  - Estrutura `Career` em 0x85AD40 (do projeto nfsu2-re + Ghidra; detalhes em NOTAS).
  - Código: var tipos `u8s`/`u32s`/`i32s` (`quantos`, `passo`, opcional `grupos`/`passo_grupo`, uma
    leitura por grupo); derivadas `conta_de` (`igual`, `chaves` + `em` = listas nomeadas de
    `jogos/<jogo>/listas.toml`) e `soma_de`. Ao vivo, mas **nunca descem** no mesmo perfil (jogo
    zerando a carreira por um instante não desbloqueia nada na volta).
  - Corrigido: marcos de 25/50/75% arredondavam o limite para baixo (meta 2 → 1/2 virava "75%").
  - Próximo (v0.3b parte 2): estado da corrida ao vivo → Street X sem bater, circuito com 30 s de
    vantagem, vencer com carro de série (propor lista depois de achar as variáveis).
- 2026-11-21: **v0.3b parte 2 feita, testada e instalada** (42 conquistas). Lista APROVADA (das 4
  propostas): 🥈 Passeio no parque (circuito comum, sem URL, vencido 30 s à frente do 2º) ·
  🥈 Mão de relojoeiro (drag vencido só com trocas perfeitas). Recusadas: "De ponta a ponta" (liderar
  todas as voltas) e "Liso" (Street X sem bater). "Vencer com carro de série" não foi proposta
  (precisaria achar as peças instaladas).
  - Tabela de resultados em 0x85036C + número da pista em 0x89E7A0 (centena = tipo). Detalhes em NOTAS.
  - Código: seção `[corrida]` no jogo.toml (formato da tabela) + derivada `corridas_de` com filtros
    `pistas`, `vencer`, `vantagem_min_s`, `trocas_perfeitas`; guardada por perfil. Conta quando a
    tabela muda e fica 1 s parada, e só se o resultado do jogador for diferente do último (sair para o
    menu mexe na tabela). `SPEEDCONQUEST_DEBUG=1` mostra cada resultado.
  - **Não testado:** corrida da carreira ao vivo (free roam tem pista 4000; cada largada grava a
    pista dela pelo mesmo pacote do Quick Race). O TESTEB agora tem uma carreira nova começada.
  - Próximo: v0.4 (janela egui com a lista).
- 2026-11-21: **v0.4 feita, testada e instalada** (commit 05391c6). `speedconquest janela` (eframe 0.37,
  glow+x11, sem wgpu): abre no perfil que avançou por último (combo para trocar), "N de 42" + contagem por
  nível, filtro Todas/Faltam/Feitas, cartão por conquista (troféu do popup em 96 px, nome em Grenze,
  descrição; feita = data local; falta = barra fina na cor do nível + "valor / meta", "recorde …" nos
  recordes sem marcos, "N% ou mais" se só houver marco, "meta X" se nada). Relê o banco a cada 2 s.
  - Banco: tabela nova `maior(jogo, perfil, var, valor, quando)` = maior valor visto de cada var de
    progresso (vigia grava a cada 2 s, só com `Avaliador::estavel()`, e ao trocar perfil/fechar);
    a janela junta com `estatistica` (progresso de antes da v0.4). `busy_timeout` 2 s.
  - `[unidade]` no jogo.toml (`texto`, `antes`, `divisor`): formatação pt-BR (US$ 12.500 · 19,9 km).
  - Menu: `~/.local/share/applications/speedconquest.desktop` (Jogos) + ação "Ver conquistas" no
    clique direito do NFSU2 (backup do .desktop em ~/.local/share/backups). Ícone
    `speedconquest.svg` no tema Plus (cópias em `extras/`). Backup do banco real antes da v0.4:
    `~/.local/share/backups/conquistas.db.antes-v0.4`.
  - Testado: Xvfb com banco falso (lista, rolagem, filtro, platina 5/41) e ponta a ponta no tela-gpu
    (TESTEB, dinheiro 4k→15k→2k na memória: maior ficou 15.000; janela aberta junto mostrou
    15.000 → 73.000 em 2 s). **Não testado:** na tela real do usuário; escala de DPI ≠ 1.
  - Próximo: a definir com o usuário.
- 2026-11-21: **renomeado para speedconquest** e publicado em github.com/1155dosilicon/speedconquest
  (público, MIT). Pasta `~/src/speedconquest`, binário `~/.local/bin/speedconquest`, banco
  `~/.local/share/speedconquest/`, variáveis `SPEEDCONQUEST_*`, menu `speedconquest.desktop`, ícone
  `speedconquest.svg`. Backups do que tinha o nome antigo em
  `~/.local/share/backups/wineconqueror-antes-rename/`. `ghidra/hud.txt` (descompilado do jogo) tirado
  do histórico e ignorado (`ghidra/*.txt`): não publicar código descompilado da EA.
  README com tudo (estado, requisitos, conquistas geradas por `ferramentas/conquistas_md.py`).
