"""Lê o bloco GCareer (0x80034A10) do GlobalB.lzc do NFSU2 e resume os prêmios.
Formatos tirados do Nikki (SpeedReflect/Nikki, Support.Underground2)."""
import struct, sys, collections

d = open(sys.argv[1], 'rb').read()
TIPOS = {0: 'circuito', 1: 'sprint', 2: 'street x', 3: 'mundo aberto', 4: 'drag', 5: 'drift'}

def chunks(buf, ini, fim):
    p = ini
    while p + 8 <= fim:
        cid, tam = struct.unpack_from('<II', buf, p)
        yield cid, p + 8, tam
        p += 8 + tam

# acha o GCareer de verdade: id + tamanho que contém sub-blocos 0x00034A1x
for off in range(len(d) - 8):
    if d[off:off+4] != bytes.fromhex('104a0380'):
        continue
    cid, tam = struct.unpack_from('<II', d, off)
    subs = {c: (p, t) for c, p, t in chunks(d, off + 8, off + 8 + tam)}
    if 0x00034A11 in subs:
        break
else:
    sys.exit('GCareer não achado')
print(f'GCareer em {off:#x}, {tam} bytes; sub-blocos: {[hex(c) for c in subs]}')

sp, st = subs[0x00034A1D]
strs = d[sp:sp+st]
def nome(o):
    return strs[o:strs.index(b'\0', o)].decode('latin1')

# Corridas: 0x88 bytes cada
rp, rt = subs[0x00034A11]
corridas = []
for i in range(rt // 0x88):
    r = d[rp + i*0x88: rp + (i+1)*0x88]
    n = nome(struct.unpack_from('<H', r, 0)[0])
    unlock, suv, _, tipo = r[12], r[13], r[14], r[15]
    respeito = struct.unpack_from('<i', r, 20)[0]
    cash = struct.unpack_from('<i', r, 48)[0]
    estagio = r[55]
    corridas.append((n, TIPOS.get(tipo, tipo), suv, cash, estagio, respeito))

# Patrocinadores: 16 bytes
pp, pt = subs[0x00034A19]
patro = []
for i in range(pt // 16):
    r = d[pp + i*16: pp + (i+1)*16]
    n = nome(struct.unpack_from('<H', r, 0)[0])
    por_vit = struct.unpack_from('<h', r, 2)[0]
    assina, potencial = struct.unpack_from('<hh', r, 12)
    patro.append((n, por_vit, assina, potencial))

# Etapas: 0x50 bytes
ep, et = subs[0x00034A18]
etapas = []
for i in range(et // 0x50):
    r = d[ep + i*0x50: ep + (i+1)*0x50]
    etapas.append((r[0], r[1], struct.unpack_from('<h', r, 2)[0], r[0x40]))

modo = sys.argv[2] if len(sys.argv) > 2 else 'resumo'
if modo == 'tudo':
    for c in corridas: print(c)
print(f'\n{len(corridas)} corridas')
por = collections.defaultdict(list)
for n, tipo, suv, cash, est, resp in corridas:
    por[est].append(cash)
for est in sorted(por):
    v = por[est]
    print(f'  etapa {est}: {len(v):3} eventos, soma US$ {sum(v):>9,}, média {sum(v)//max(len(v),1):>6,}, min {min(v):>6,}, max {max(v):>6,}')
print(f'  TOTAL: US$ {sum(c[3] for c in corridas):,}')
print('\nPatrocinadores (nome, por vitória, assinatura, bônus potencial):')
for p in patro: print('  ', p)
print('\nEtapas (id, patrocinadores p/ escolher, prêmio outrun, máx outruns):')
for e in etapas: print('  ', e)
