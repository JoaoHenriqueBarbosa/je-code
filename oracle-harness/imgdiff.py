"""Diferença entre duas capturas (H26): pixels diferentes, diferença média e máxima por canal, e a
caixa que contém as diferenças. Uso: python imgdiff.py a.png b.png [saida_diff.png]"""
import sys

import numpy as np
from PIL import Image


def main():
    a = np.asarray(Image.open(sys.argv[1]).convert('RGB'), dtype=np.int16)
    b = np.asarray(Image.open(sys.argv[2]).convert('RGB'), dtype=np.int16)
    if a.shape != b.shape:
        print(f'tamanhos diferentes: {a.shape} contra {b.shape}')
        return 1
    d = np.abs(a - b)
    per_pixel = d.max(axis=2)
    changed = per_pixel > 0
    n = int(changed.sum())
    total = changed.size
    print(f'{n} de {total} pixels diferentes ({100.0 * n / total:.3f}%), média {d.mean():.4f}, máx {int(d.max())}')
    if n:
        ys, xs = np.nonzero(changed)
        print(f'caixa das diferenças: x {xs.min()}..{xs.max()}, y {ys.min()}..{ys.max()}')
        if len(sys.argv) > 3:
            Image.fromarray((changed * 255).astype(np.uint8)).save(sys.argv[3])
    return 0


if __name__ == '__main__':
    sys.exit(main())
