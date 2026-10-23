"""Relatório da verificação visual dos widgets (H26/H11): compara, por caso e tema, o PNG do oráculo
(`wip/scratch/parity/`) com o do je-code (`wip/scratch/parity-je/`) e os retângulos dos widgets nos dois TSV.

Uso: uv run --with numpy --with pillow python3 -I oracle-harness/parity_report.py [--regions] [--oracle=pasta] [--ours=pasta] [--diff=pasta] [caso ...]
Saída: uma linha por caso e tema com pixels diferentes/total, diferença máxima, pixels acima de 2 e de 16 níveis, e as
diferenças de retângulo (posição e tamanho acima de 0,5 px). Grava `wip/scratch/parity-diff/<caso>-<tema>.png`
(oráculo | je-code | diferença amplificada lado a lado) para abrir com os olhos. Com `--regions`, repete a conta por
widget (cada nome do TSV), com a caixa do retângulo do oráculo.
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ORACLE, OURS, OUT = ROOT / 'wip/scratch/parity', ROOT / 'wip/scratch/parity-je', ROOT / 'wip/scratch/parity-diff'


def read_tsv(path):
    rects = {}
    for line in path.read_text().splitlines():
        f = line.split('\t')
        if not f[0].startswith('#'):
            rects[f[0]] = tuple(float(x) for x in f[1:5])
    return rects


def stats(a, b):
    d = np.abs(a - b)
    per = d.max(axis=2)
    return int((per > 0).sum()), per.size, int(d.max()), int((per > 16).sum()), int((per > 2).sum())


def main():
    global ORACLE, OURS, OUT
    args = sys.argv[1:]
    for a in args:
        if a.startswith('--oracle='): ORACLE = ROOT / a[9:]
        if a.startswith('--ours='): OURS = ROOT / a[7:]
        if a.startswith('--diff='): OUT = ROOT / a[7:]
    regions = '--regions' in args
    only = [a for a in args if not a.startswith('--')]
    OUT.mkdir(parents=True, exist_ok=True)
    names = sorted({p.name.rsplit('-', 1)[0] for p in ORACLE.glob('*.png')})
    for name in names:
        if only and name not in only:
            continue
        for theme in ('dark', 'light'):
            po, pj = ORACLE / f'{name}-{theme}.png', OURS / f'{name}-{theme}.png'
            if not po.exists() or not pj.exists():
                continue
            a = np.asarray(Image.open(po).convert('RGB'), dtype=np.int16)
            b = np.asarray(Image.open(pj).convert('RGB'), dtype=np.int16)
            if a.shape != b.shape:
                print(f'{name}-{theme}: tamanhos diferentes {a.shape} contra {b.shape}')
                continue
            n, total, mx, over, over2 = stats(a, b)
            print(f'{name}-{theme}: {n}/{total} pixels diferentes ({100 * n / total:.3f}%), máx {mx}, >2: {over2}, >16: {over}')
            amplified = np.clip(np.abs(a - b) * 4, 0, 255).astype(np.uint8)
            strip = np.concatenate([a.astype(np.uint8), b.astype(np.uint8), amplified], axis=1)
            Image.fromarray(strip).save(OUT / f'{name}-{theme}.png')
            ro, rj = read_tsv(ORACLE / f'{name}-{theme}.tsv'), read_tsv(OURS / f'{name}-{theme}.tsv')
            for key, r in rj.items():
                o = ro.get(key)
                if o is None:
                    continue
                moved = [abs(x - y) > 0.5 for x, y in zip(o, r)]
                if any(moved):
                    print(f'    retângulo {key}: oráculo x={o[0]:.1f} y={o[1]:.1f} w={o[2]:.1f} h={o[3]:.1f}, je-code x={r[0]:.1f} y={r[1]:.1f} w={r[2]:.1f} h={r[3]:.1f}')
            if regions:
                for key, o in ro.items():
                    x0, y0 = int(np.floor(o[0])), int(np.floor(o[1]))
                    x1, y1 = int(np.ceil(o[0] + o[2])), int(np.ceil(o[1] + o[3]))
                    ra, rb = a[y0:y1, x0:x1], b[y0:y1, x0:x1]
                    if ra.size == 0:
                        continue
                    rn, rt, rm, rover, rover2 = stats(ra, rb)
                    print(f'    {key}: {rn}/{rt} diferem, máx {rm}, >2: {rover2}, >16: {rover}')


if __name__ == '__main__':
    main()
