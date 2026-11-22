# NFSU2 — notas

## Endereços (SPEED2.EXE sha256 f9dd86c0…, 32-bit, base 0x400000)
| var | endereço | tipo | obs |
|---|---|---|---|
| perfil | `SPEED2.EXE+0x43A9E0` | texto 16 | "No Profile" antes de carregar um save |
| dinheiro | `SPEED2.EXE+0x461E74` | i32 | conferido gravando 12345 e vendo na garagem |
| posicao | `SPEED2.EXE+0x42BE80` | vec3 (m) | x, y, z (z = altura). Derivada = velocímetro (36 mph HUD × 34–38 calculado). Free roam e Quick Race |
| drift_total | `[[[[[SPEED2.EXE+0x4900AC]+4]+0x1C]+0x1D4]+0]+0x2C` | f32 | total do evento de drift; só existe em drift |
| drift_melhor | mesma cadeia, `+0x44` | f32 | melhor drift único do evento |
| cima | `[[SPEED2.EXE+0x4900AC]+4]+0xB0` | vec3 | vetor "pra cima" do carro (z=1 de pé). Cambalhotas |

### Objeto do carro (achado 2026-11-21, v0.3)
`carro = [[0x8900AC]+4]` (jogador 0 → carro; muda de endereço a cada corrida, a cadeia acompanha).
`+0x50`/`+0x60` posição (duas cópias), `+0x70` velocidade em m/s (vetor, suave — melhor que derivar
da posição), `+0x80` provavelmente velocidade angular, `+0x90/+0xA0/+0xB0` linhas da orientação
(frente, lado, cima), `+0x130…` matrizes das 4 rodas (`+0x168 + 0x40*i` = z local da roda: ~0,05
estendida … 0,288 batente). `0x82BE80` é cópia da matriz de desenho `[0x82BB40]+0x60`
(Ghidra: `FUN_0046d7e0`), não da física: a z dela **oscila** quadro a quadro no chão.
- **Escrever na física não funciona**: z, velocidade (+0x70) e matriz são reescritos todo quadro a
  partir de algo que não achei (corpo rígido em outro lugar). Não dá para "lançar" o carro.
- **Salto (resolvido, ver abaixo)**: só a aceleração vertical não separa chão de ar (no chão vai de
  −25 a +23 m/s², e passa de −13 no topo de morro com as rodas no chão).
  `ferramentas/grava_pos.py` (posição a 60 Hz) e o `grava_carro` da sessão (pos/vel/cima/rodas).
- **Pausado, o jogo ainda copia uma posição velha para `0x82BE80`** (não é só parar): escrever a
  posição para simular velocidade **sustentada** falha (o vigia vê teletransportes). O teste do
  "Pé embaixo" foi com o carro andando de verdade e um binário de teste com limite 80 km/h / meta 5 s.
  O vetor `cima` pausado fica parado: escrever nele a 120 Hz simula cambalhotas.

### Como foram achados (2026-11-21)
- **Velocidade escalar não existe pronta**: varredura de floats (pausando com Escape *segurado*:
  o jogo lê o teclado por DirectInput, `xdotool key` rápido demais não pega) não achou nada que
  batesse com o HUD. O jogo guarda o vetor. Solução: achar a **posição** (varredura parado/andando,
  `ferramentas/varre.py`) e derivar velocidade/odômetro/salto. Há dezenas de cópias da posição
  (câmera, marcas de pneu em 0x8A1F50…); a de `0x82BE80` é estática e bate com o HUD.
- **Drift**: o ExtraOptions (github.com/ExOptsTeam/NFSU2ExOpts) mexe em `sub_4AB780` (texto do drift
  no HUD). Ghidra: chamada por `FUN_004c85d0`, que lê `[[[[jogador+4]+0x1C]+0x1D4]]` +0x44 (linha
  "Best") e +0x2C (total). `jogador = [0x8900AC + 4*i]` (`_PlayersByNumber` do ExOpts).
  Conferido nos adversários (mesma estrutura, passo 0x420): +0x2C = placar final do evento.
  Os adversários pontuam por simulação (lista em 0x864660, nós de 0x20 B com float em +0x10).
- **Teste sem carreira**: `UnlockAllThings` (flag de debug do motor da EA) em `0x838464`.
  Gravar 1 lá na memória do jogo de teste destrava todas as pistas do Quick Race. Nada é instalado.
  Fazer isso com o jogo de teste, nunca no jogo do usuário (estraga a carreira dele).
- Ghidra: projeto em `ghidra/proj` (fora do git), script `ghidra/scripts/Decomp.java`
  (`ghidra-headless ghidra/proj nfsu2 -process SPEED2.EXE -noanalysis -readOnly -scriptPath ghidra/scripts -postScript Decomp.java 4c85d0`).

### Limitações conhecidas
- Odômetro: **o replay move a mesma posição** (conferido 2026-11-21: ~27 mph no replay) → assistir
  replay soma km. Aceito (pouco perto das metas). `0x89E7D9` NÃO é flag de replay (vale 1 correndo
  também). Para corrigir: varrer u32 corrida→replay→corrida (mudou/igual) — cada ciclo é uma corrida
  inteira (~3 min no drift, que acaba por tempo). Velocidade no replay não importa (repete marca já feita).
- Pontuar drift com xdotool às cegas é muito difícil ("Too Slow!", bate na parede): o teste de ponta
  a ponta das conquistas de drift é gravando o float na estrutura pela memória.

### Teste de ponta a ponta da v0.2 (2026-11-21, tela-gpu, banco de teste, perfil TESTEB)
- Odômetro: semeado 99.990 m no banco (`insert into estatistica`), dirigir 10 m → "Rodando a cidade".
- Drift: gravar float em `S+0x2C`/`S+0x44` na corrida de drift → 3 conquistas. O placar do jogo passou
  a mostrar 1º com 100.500 → confirma que é a pontuação real do jogador.
- Velocidade: com o jogo **pausado** (Escape segurado; a pausa para de reescrever a posição), gravar
  `0x82BE80` andando 100 m/s a 60 Hz → 200/300/350 em sequência (pico 363 km/h), 3 popups na fila.
- Captura com popup: pedir `GAMESCOPECTRL_REQUEST_SCREENSHOT 3` e **esperar o `/tmp/gamescope.png`
  mudar** antes de copiar (senão pega o arquivo antigo).

### Saltos (2026-11-21, gravação de 3 saltos do usuário no morro do centro, com o ok dele; apagada)
- **No ar = as 4 rodas paradas** (`+0x168 + 0x40*i`, todas no fim de curso: −0,028 no Integra)
  **e a velocidade vertical (`+0x78`) caindo** a cada leitura. A 30 Hz: 0 falso positivo, só perde o
  1º quadro. Não depende do fim de curso de cada carro (só de "parado"). Decola com 3 leituras no ar,
  pousa com 2 no chão; distância = horizontal entre decolagem e pouso.
- **Câmera lenta nos saltos grandes**: o jogo roda a ~41% (posição e velocidade por segundo real).
  Gravidade medida no relógio real −6,6 m/s² (≈ −16 em tempo de jogo). Por isso a medida é metros.
- Saltos dele: 43 m (132 km/h, ~3 s reais no ar), 33 m e 31 m (~125 km/h). Solavancos: 3–13 m.
- Teste com gravação: CSV `t,px,py,pz,vx,vy,vz,r1..r4` →
  `SPEEDCONQUEST_GRAVACAO=arq.csv cargo nextest run --run-ignored only saltos_da_gravacao --no-capture`.
- Ponta a ponta: jogo pausado, escrever no objeto do carro `+0x60` (pos), `+0x70` (vel), rodas fixas
  em −0,028 andando 30 m/s por 1,85 s → Decolagem + Voo rasante; 45 m/s → Sem asas.

### Teste de ponta a ponta da v0.3 (2026-11-21, tela-gpu, banco de teste, perfil TESTEB)
- Cambalhotas: jogo pausado, gravar `cima` = (0, sen g, cos g) girando 720°/s até 1.080° →
  Capotou!, Cambalhota dupla, Circo de Bayview e (semeado `voltas`=24) Dublê.
- Pé embaixo + platina: as 12 antigas semeadas em `desbloqueio`; binário de teste (limite 80,
  meta 5 s, TOMLs restaurados depois), segurar Up na reta da largada → Pé embaixo → platina.

## Carreira na memória (v0.3b, 2026-11-21)
Fonte: github.com/yugecin/nfsu2-re (`docs/vars.html`, `structs.html`, `funcs.html`; nomes de funções
e structs) + Ghidra. `profileData` = `0x83A9D0` (o nome do perfil em +0x10 é a nossa var `perfil`);
`Career` = profileData + 0x20370 = **`0x85AD40`** (dinheiro em +0x7134 = nossa var `dinheiro`).
| Career + | o quê |
|---|---|
| 0x72D8 + 0x104·i | `CareerStage` i (0 = prólogo, 1–5 = Stage 1–5; 6/7 vazias). +0xFC concluída, +0xDC outruns **vencidos**, +0xE8 patrocinador (ponteiro), +0xE4 corridas terminadas, +0x0/+0x8C/+0x9C hashes de vitórias mundo/patrocinador/URL |
| 0x7AF8 | etapa atual |
| 0x7AFC / 0x7B00 | nº usado / 384 registros de 8 B: +0 hash (corrida, capa ou loja), +4 visível no mapa, +6 = 1 feito/vencido (5 = loja) |
| 0x872C | corridas disputadas |
- `careerStats` (0x835C00) **não** é ao vivo: só é recalculado ao abrir a tela de status
  (`Career::UpdateCareerStatsObject` 0x528A60, chamado só por 0x4E1B80). Por isso contamos nós,
  com a mesma regra dela: corrida vencida = registro com +6 == 1; tipo pela tabela fixa.
- Tabelas fixas (do GlobalB, com o mod): corridas `[0x8383E0]`, n em 0x8383E4, 0x88 B (+8 hash,
  +0xF tipo, +0x34 recompensa); capas `[0x83840C]`, n em 0x838408, 0x40 B (+0x20 hash,
  +0x24 tipo 1/3 revista, 2/4 DVD). `ferramentas/nfsu2_listas.py` gera `listas.toml` delas.
- `Career::DoPostRaceThings` (0x541420) marca +6 = 1 e chama `IncrementStageIfFinished` (0x532C60).
  `Career::OnOutrunFinish` (0x541320) soma 1 em +0xDC só se venceu (e só com 0x850078 == 1).
- Ponta a ponta (tela-gpu, perfil TESTEB, banco de teste): escrever nos registros **depois** do
  contador do jogo (entradas 100+, o jogo ignora), nas flags das etapas e nos outruns; patrocínio =
  ponteiro de verdade `[0x838414]` (careerSponsors). As 18 dispararam; troca UNIX↔TESTEB sem disparo.
  Script da sessão: `escreve.py` (não versionado; repetir pelo método acima).

## Prêmios da carreira (extraídos de `GLOBAL/GlobalB.lzc`, 2026-11-21)
Leitor: `ferramentas/nfsu2_carreira.py <GlobalB.lzc> [tudo]` (formato do Nikki/SpeedReflect,
bloco GCareer 0x80034A10: corridas 0x34A11 de 0x88 B com prêmio no desloc. 48 e etapa no 55;
patrocinadores 0x34A19 de 16 B; etapas 0x34A18 de 0x50 B). O GlobalB.lzc deste PC está
descompactado (salvo pelo Binary com o mod do Integra); o original é JDLZ.

| etapa | eventos | soma | média | menor | maior |
|---|---|---|---|---|---|
| 0 (prólogo) | 5 | 800 | 160 | 0 | 400 |
| 1 | 5 | 1.250 | 250 | 250 | 250 |
| 2 | 35 | 22.400 | 640 | 300 | 1.100 |
| 3 | 48 | 38.700 | 806 | 350 | 1.500 |
| 4 | 62 | 185.800 | 2.996 | 1.200 | 8.000 |
| 5 | 69 | 299.000 | 4.333 | 2.000 | 10.000 |
| **total** | **224** | **547.950** | | | |

Por categoria: patrocinador (SPON) 44 eventos / 225.500 · normais 101 / 126.750 ·
URL 24 / 108.300 · `S?_H_*` 32 / 71.800 · SUV 13 / 13.550.

Extras fora da tabela: assinatura de patrocinador (etapa 2: 1.275–1.500; etapa 3: 1.775–2.500;
etapa 4: 5.200–7.000; etapa 5: 13.000–15.000), bônus por vitória de patrocinador (825–3.225) e
outrun (100 nas etapas 0–3, 250 na 4, 400 na 5).
Estimativa de uma carreira completa: ~US$ 600 mil de ganhos. O usuário aprovou as metas por esta
tabela e **não quer monitoramento** do jogo dele (histórico desligado). Não confirmado: se 2º/3º
lugar pagam e se dá para repetir eventos.

## Resultado da corrida (v0.3b parte 2, 2026-11-21, Quick Race no tela-gpu, perfil TESTEB)
Achado pela tela de estatísticas pós-corrida: os hashes de texto "FINISH TIME" (0xF9D7F4C2),
"LAPS LED" (0xADBFD917), "PERFECT SHIFTS" (0x680AC597) aparecem como constantes em
FUN_004BF9E0…FUN_004BFD00 (uma função por linha da tela, vtables em 0x79513C+0x40·k), que leem
`[linha+0x4C]` = registro do corredor. Os registros ficam em **0x85036C**, 0x224 B cada,
ordenados pela colocação; nº de corredores = soma dos 4 contadores em 0x89E7C0.
| registro + | o quê |
|---|---|
| 0x0 | colocação (1 = venceu) |
| 0x4 | id do corredor (0 = jogador; IA = 100, 101…). A tela destaca a linha cujo corredor é humano (FUN_004C0300) |
| 0x8 | status: 1 terminou, 3 eliminado (knockout), 6/7 venceu/perdeu outrun, 8 motor estourado, 9 destruído, 0xB tempo esgotado, 0xC desconectado |
| 0xC | tempo final em 1/4000 s (36,37 s na tela = 145.480) |
| 0x10 | melhor volta (1/4000 s) |
| 0xCC + 4·v | colocação no fim da volta v; 0xF4 voltas na liderança (FUN_00541B40 conta as voltas em 1º) |
| 0x10C / 0x110 | velocidade máxima / média (f32, m/s) |
| 0x124 / 0x128 | trocas de marcha / trocas perfeitas (tela "3/5" = +0x128 / +0x124, conferido escrevendo) |
- **Quando é escrita**: ~3 s depois que o jogador termina (ou é eliminado/estoura o motor), uma vez,
  por FUN_0054CCA0 → FUN_00515DB0 (copia status/tempo do "racer info" `[[carro]+0x1C]`: +0x10 status
  em short, +0x198 tempo). Quem ainda corre **já entra com status 1 e tempo projetado** (drag com motor
  estourado aos 9,8 s: os 3 adversários vieram com 80–83 s). Restart e sair **não apagam** a tabela;
  sair para o menu mexe em algum campo (por isso o avaliador compara o resultado do jogador, não só a tabela).
- Ao vivo: status do jogador em `[[[0x8900AC]+4]+0x1C]+0x10` (short), 0 correndo; zera a cada largada
  (Restart recria o carro: o ponteiro fica nulo por um instante).
- **Tipo da corrida**: `0x89E7A0` = número da pista, gravado a cada largada a partir do pacote de
  parâmetros (FUN_00580130 → FUN_0053FEB0). Centena = tipo, igual na tabela de corridas da carreira:
  40 circuito, 41 sprint (10 drifts downhill da carreira também usam 41xx), 42 e 44 drag, 43 drift,
  46 Street X, 47 URL. **Free roam = 4000** (só importa se uma corrida da carreira não gravasse a pista;
  não testado ao vivo na carreira). Flags da mesma largada: 0x89E7D8 drag, 0x89E7D9 drift,
  0x89E7E1 URL, 0x89E7E3 Street X, 0x89E7B4 = 1 ponto a ponto (sprint/drag), 0x89E7BC voltas.
  `0x83AAB4` é das opções do Quick Race (não serve na carreira).
- Batida não existe no jogo (só "Totaled"). Medido: muro a 90 km/h = velocidade (+0x70 do carro)
  de 24,9 → 3,7 m/s em um quadro (~1.450 m/s²); freada forte ~10–15 m/s². Detector não feito
  ("Liso", Street X sem bater, não foi aprovado).
- Câmbio: com "Auto" escolhido, no drag o carro ficou em 1ª e estourou o motor (não achei as teclas
  de troca; teste das conquistas foi escrevendo na tabela).
- Ponta a ponta (tela-gpu, banco de teste): escrever a tabela com `escreve_res.py` da sessão
  (linhas `id,status,tempo_s,trocas,perfeitas` na ordem de chegada) durante a corrida certa:
  drag 4201 4/5 → nada, 5/5 → Mão de relojoeiro; circuito 4021 com 29 s → nada, 30,5 s → Passeio no
  parque (popup visto na captura do gamescope). A tela "Post Race Stats" do jogo mostrou os valores escritos.
