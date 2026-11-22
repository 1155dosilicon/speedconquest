#!/usr/bin/env python3
"""Gera jogos/nfsu2/listas.toml lendo as tabelas fixas da carreira na memória do
NFSU2 aberto (SPEED2.EXE, qualquer tela depois do menu; não escreve nada).

As tabelas vêm do GlobalB.lzc já carregado, então refletem os mods instalados.
Fontes (nfsu2-re de yugecin + Ghidra, Career::UpdateCareerStatsObject 0x528A60):
- careerRaces: ponteiro em 0x8383E0, quantidade em 0x8383E4, itens de 0x88 B.
  +0x08 hash, +0x0F tipo (0 circuito, 1 sprint, 2 Street X, 3 prólogo/outro,
  4 drag, 5 drift), +0x34 recompensa (0 prólogo, 1 patrocinador, 2 URL, 3 normal).
  O jogo conta como URL toda corrida com recompensa 2, seja qual for o tipo.
- vitrines (capas): ponteiro em 0x83840C, quantidade em 0x838408, itens de 0x40 B.
  +0x20 hash, +0x24 tipo (1/3 revista, 2/4 DVD), +0x25 etapa.

Uso: python3 ferramentas/nfsu2_listas.py > jogos/nfsu2/listas.toml
"""

import struct
import subprocess

pid = int(subprocess.check_output(["pgrep", "-x", "SPEED2.EXE"]).split()[0])
mem = open(f"/proc/{pid}/mem", "rb")


def ler(a, n):
    mem.seek(a)
    return mem.read(n)


def u32(a):
    return struct.unpack("<I", ler(a, 4))[0]


def u8(a):
    return ler(a, 1)[0]


listas = {k: [] for k in ["circuito", "sprint", "streetx", "drag", "drift", "url", "revista", "dvd"]}
TIPO = {0: "circuito", 1: "sprint", 2: "streetx", 4: "drag", 5: "drift"}
base, n = u32(0x8383E0), u32(0x8383E4)
for i in range(n):
    r = base + 0x88 * i
    recompensa, tipo = u8(r + 0x34), u8(r + 0x0F)
    if recompensa == 0:
        continue  # prólogo: o jogo não conta nas estatísticas
    nome = "url" if recompensa == 2 else TIPO.get(tipo)
    if nome is None:
        raise SystemExit(f"corrida {i}: tipo {tipo} desconhecido")
    listas[nome].append(u32(r + 8))
base, n = u32(0x83840C), u32(0x838408)
for i in range(n):
    v = base + 0x40 * i
    listas["revista" if u8(v + 0x24) in (1, 3) else "dvd"].append(u32(v + 0x20))

print("# Gerado por ferramentas/nfsu2_listas.py lendo o jogo aberto (não editar à mão).")
print("# Hashes das corridas da carreira por tipo e das capas (revista/DVD).")
for nome, hs in listas.items():
    print(f"\n# {len(hs)} itens")
    print(f"{nome} = [")
    for j in range(0, len(hs), 6):
        print("    " + " ".join(f"0x{h:08X}," for h in hs[j : j + 6]))
    print("]")
