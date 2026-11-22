#!/usr/bin/env python3
"""grava_pos — grava a posição do carro (NFSU2, 0x82BE80) a ~250 Hz em CSV.

Uso: python3 ferramentas/grava_pos.py SEGUNDOS saida.csv
Só grava quando a posição muda (= um quadro novo do jogo). Colunas: t, x, y, z.
"""
import struct
import subprocess
import sys
import time

ENDERECO = 0x82BE80
seg, saida = float(sys.argv[1]), sys.argv[2]
pid = int(subprocess.check_output(["pgrep", "-x", "SPEED2.EXE"]).split()[0])
with open(f"/proc/{pid}/mem", "rb", buffering=0) as m, open(saida, "w") as f:
    f.write("t,x,y,z\n")
    ant = None
    t0 = time.monotonic()
    while (t := time.monotonic() - t0) < seg:
        m.seek(ENDERECO)
        p = struct.unpack("<3f", m.read(12))
        if p != ant:
            f.write(f"{t:.4f},{p[0]:.4f},{p[1]:.4f},{p[2]:.4f}\n")
            ant = p
        time.sleep(0.004)
