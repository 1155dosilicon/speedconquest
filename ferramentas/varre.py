#!/usr/bin/env python3
"""varre — varredura de memória estilo Cheat Engine, sem root (lê /proc/PID/mem).

Uso (estado fica em $VARRE_DIR, padrão ./varre-estado):
  varre.py novo PROC TIPO COND...   começa: todas as posições alinhadas que passam em COND
  varre.py f COND...                filtra os candidatos atuais (compara com a leitura anterior)
  varre.py ver [N]                  mostra até N candidatos (endereço, valor anterior, atual)
  varre.py le PROC TIPO ENDERECO... lê valores avulsos

TIPO: f32 | i32 | u32 | f64
COND: entre A B | igual X | mais | menos | mudou | igual_ant | quase_ant T | mudou_mais T |
      mais_que X | menos_que X
Só olha regiões graváveis abaixo de 4 GB (jogos 32-bit no Wine).
Rodar com: uv run --with numpy ferramentas/varre.py ...
"""

import os
import subprocess
import sys

import numpy as np

TIPOS = {"f32": np.float32, "i32": np.int32, "u32": np.uint32, "f64": np.float64}
DIR = os.environ.get("VARRE_DIR", "varre-estado")


def pid_de(proc):
    if proc.isdigit():
        return int(proc)
    return int(subprocess.check_output(["pgrep", "-x", proc]).split()[0])


def regioes(pid):
    with open(f"/proc/{pid}/maps") as f:
        for linha in f:
            partes = linha.split()
            a, b = (int(x, 16) for x in partes[0].split("-"))
            if partes[1].startswith("rw") and b <= 1 << 32:
                yield a, b


def le_tudo(pid, tipo):
    """Devolve (enderecos, valores) de todas as posições alinhadas."""
    dt = np.dtype(TIPOS[tipo])
    ends, vals = [], []
    with open(f"/proc/{pid}/mem", "rb", buffering=0) as m:
        for a, b in regioes(pid):
            try:
                m.seek(a)
                dados = m.read(b - a)
            except OSError:
                continue
            n = len(dados) // dt.itemsize
            if n == 0:
                continue
            vals.append(np.frombuffer(dados[: n * dt.itemsize], dtype=dt))
            ends.append((a + np.arange(n, dtype=np.uint64) * dt.itemsize).astype(np.uint32))
    return np.concatenate(ends), np.concatenate(vals)


def le_enderecos(pid, tipo, ends):
    """Lê só os endereços pedidos, agrupando por região (rápido para poucos candidatos)."""
    dt = np.dtype(TIPOS[tipo])
    if len(ends) > 2_000_000:
        # Muitos candidatos: mais barato reler tudo e casar os endereços.
        todos_e, todos_v = le_tudo(pid, tipo)
        i = np.searchsorted(todos_e, ends)
        i = np.minimum(i, len(todos_e) - 1)
        ok = todos_e[i] == ends
        return np.where(ok, todos_v[i], 0).astype(dt), ok
    out = np.zeros(len(ends), dtype=dt)
    ok = np.zeros(len(ends), dtype=bool)
    ordem = np.argsort(ends)
    e = ends[ordem].astype(np.uint64)
    with open(f"/proc/{pid}/mem", "rb", buffering=0) as m:
        for a, b in regioes(pid):
            i0, i1 = np.searchsorted(e, [a, b])
            if i0 == i1:
                continue
            lo, hi = int(e[i0]), int(e[i1 - 1]) + dt.itemsize
            try:
                m.seek(lo)
                dados = np.frombuffer(m.read(hi - lo), dtype=np.uint8)
            except OSError:
                continue
            desl = (e[i0:i1] - lo).astype(np.int64)
            idx = desl[:, None] + np.arange(dt.itemsize)
            out[ordem[i0:i1]] = dados[idx].copy().view(dt).ravel()
            ok[ordem[i0:i1]] = True
    return out, ok


def condicao(args, atual, anterior):
    c = args[0]
    with np.errstate(invalid="ignore"):
        if c == "entre":
            return (atual >= float(args[1])) & (atual <= float(args[2]))
        if c == "igual":
            return atual == float(args[1])
        if c == "mais_que":
            return atual > float(args[1])
        if c == "menos_que":
            return atual < float(args[1])
        if anterior is None:
            sys.exit(f"condição {c} precisa de leitura anterior")
        if c == "mais":
            return atual > anterior
        if c == "menos":
            return atual < anterior
        if c == "mudou":
            return atual != anterior
        if c == "igual_ant":
            return atual == anterior
        if c == "quase_ant":
            return np.abs(atual - anterior) <= float(args[1])
        if c == "mudou_mais":
            return np.abs(atual - anterior) > float(args[1])
    sys.exit(f"condição desconhecida: {c}")


def salva(pid, tipo, ends, vals, ant=None):
    os.makedirs(DIR, exist_ok=True)
    np.savez(os.path.join(DIR, "estado.npz"), pid=pid, tipo=tipo, ends=ends.astype(np.uint32),
             vals=vals)
    print(f"{len(ends)} candidatos")


def main():
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    if a[0] == "novo":
        pid, tipo = pid_de(a[1]), a[2]
        ends, vals = le_tudo(pid, tipo)
        if tipo.startswith("f"):
            fin = np.isfinite(vals)
            ends, vals = ends[fin], vals[fin]
        k = condicao(a[3:], vals, None) if len(a) > 3 else np.ones(len(vals), bool)
        salva(pid, tipo, ends[k], vals[k])
    elif a[0] == "f":
        st = np.load(os.path.join(DIR, "estado.npz"))
        pid, tipo = int(st["pid"]), str(st["tipo"])
        ends, ant = st["ends"], st["vals"]
        atual, ok = le_enderecos(pid, tipo, ends)
        k = ok & condicao(a[1:], atual, ant)
        salva(pid, tipo, ends[k], atual[k], ant[k])
    elif a[0] == "ver":
        st = np.load(os.path.join(DIR, "estado.npz"))
        n = int(a[1]) if len(a) > 1 else 30
        pid, tipo = int(st["pid"]), str(st["tipo"])
        atual, _ = le_enderecos(pid, tipo, st["ends"][:n])
        for e, v0, v in zip(st["ends"][:n], st["vals"][:n], atual):
            print(f"0x{int(e):08X}  {v0!s:>14}  {v!s:>14}")
        print(f"({len(st['ends'])} no total)")
    elif a[0] == "le":
        pid, tipo = pid_de(a[1]), a[2]
        ends = np.array([int(x, 16) for x in a[3:]], dtype=np.uint64)
        vals, _ = le_enderecos(pid, tipo, ends)
        for e, v in zip(ends, vals):
            print(f"0x{int(e):08X}  {v}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
