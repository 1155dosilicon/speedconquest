#!/usr/bin/env python3
"""Imprime a lista de conquistas de um jogo em Markdown (para o README).

uso: python3 ferramentas/conquistas_md.py jogos/nfsu2/conquistas.toml
"""
import sys
import tomllib

NIVEL = {"bronze": "🥉 bronze", "prata": "🥈 prata", "ouro": "🥇 ouro", "platina": "🏆 platina"}

with open(sys.argv[1], "rb") as f:
    cs = tomllib.load(f)["conquista"]
cont = {}
for c in cs:
    cont[c["nivel"]] = cont.get(c["nivel"], 0) + 1
print(f"**{len(cs)} conquistas:** " + " · ".join(f"{NIVEL[n]} {cont[n]}" for n in NIVEL if n in cont))
print()
print("| | Nome | Como desbloquear | Avisos 25/50/75% |")
print("|---|---|---|---|")
for c in cs:
    marcos = "—" if "progresso" not in c or c.get("marcos") is False else "sim"
    print(f"| {NIVEL[c['nivel']].split()[0]} | **{c['nome']}** | {c['descricao']} | {marcos} |")
